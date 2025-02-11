use serde_json::Value;

pub trait ToUsize {
    fn to_usize(&self) -> Result<usize, &'static str>;
}

impl ToUsize for Value {
    fn to_usize(&self) -> Result<usize, &'static str> {
        match self {
            Value::String(s) if s == "disabled" => Err("disabled"),
            Value::String(s) => s.parse::<usize>().map_err(|_| "default"),
            Value::Number(n) if n.is_u64() => Ok(n.as_u64().unwrap() as usize),
            _ => Err("default"),
        }
    }
}

impl ToUsize for u64 {
    fn to_usize(&self) -> Result<usize, &'static str> {
        Ok(*self as usize)
    }
}

pub async fn parse_upload_limit<T: ToUsize>(limit_val: &Option<T>) -> Result<usize, &'static str> {
    match limit_val {
        Some(val) => val.to_usize(),
        None => Err("default"),
    }
}
