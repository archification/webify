// Read-only client for the CMS NPI Registry public lookup API
// (https://npiregistry.cms.hhs.gov/api-page). No API key required. Used to
// show registry info on the portal dashboard for the NPI number a hospital
// account registered with.

use serde::{Deserialize, Serialize};
use std::time::Duration;

const NPI_REGISTRY_URL: &str = "https://npiregistry.cms.hhs.gov/api/";

pub fn is_valid_format(npi: &str) -> bool {
    npi.len() == 10 && npi.chars().all(|c| c.is_ascii_digit())
}

#[derive(Debug, Deserialize)]
struct NpiApiResponse {
    #[serde(default)]
    results: Vec<NpiResult>,
}

#[derive(Debug, Deserialize)]
struct NpiResult {
    number: String,
    enumeration_type: String,
    basic: NpiBasic,
    #[serde(default)]
    addresses: Vec<NpiAddress>,
    #[serde(default)]
    taxonomies: Vec<NpiTaxonomy>,
}

#[derive(Debug, Default, Deserialize)]
struct NpiBasic {
    first_name: Option<String>,
    last_name: Option<String>,
    credential: Option<String>,
    organization_name: Option<String>,
    status: Option<String>,
}

#[derive(Debug, Deserialize)]
struct NpiAddress {
    address_purpose: Option<String>,
    address_1: Option<String>,
    city: Option<String>,
    state: Option<String>,
    postal_code: Option<String>,
    telephone_number: Option<String>,
}

#[derive(Debug, Deserialize)]
struct NpiTaxonomy {
    desc: Option<String>,
    #[serde(default)]
    primary: bool,
}

/// Flattened, display-ready shape handed straight to the Tera context —
/// `portal-dashboard.html` doesn't need to know about the registry's raw
/// response shape or juggle nested Options.
#[derive(Debug, Clone, Serialize)]
pub struct NpiInfo {
    pub number: String,
    pub name: String,
    pub entity_type: String,
    pub credential: Option<String>,
    pub status_display: String,
    pub primary_taxonomy: Option<String>,
    pub address_line: Option<String>,
    pub city_state_zip: Option<String>,
    pub phone: Option<String>,
}

/// `Ok(None)` means the registry was reachable but has no record for this
/// number (e.g. it was deactivated, or was never a valid NPI). `Err` means
/// the lookup itself failed (network/timeout/parse) — the caller should
/// treat that as "try again later", not "no such provider".
pub async fn lookup(npi: &str) -> Result<Option<NpiInfo>, String> {
    if !is_valid_format(npi) {
        return Err("Invalid NPI format".to_string());
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| e.to_string())?;

    // `reqwest`'s `query()` builder needs the "query" cargo feature, which
    // isn't enabled for this vendored build — `npi` is already validated as
    // 10 ASCII digits above, so a plain format! is just as safe here.
    let url = format!("{}?number={}&version=2.1", NPI_REGISTRY_URL, npi);
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        return Err(format!("NPI registry returned HTTP {}", resp.status()));
    }

    let parsed: NpiApiResponse = resp.json().await.map_err(|e| e.to_string())?;
    let Some(result) = parsed.results.into_iter().next() else {
        return Ok(None);
    };

    let is_org = result.enumeration_type == "NPI-2";
    let name = if is_org {
        result.basic.organization_name.clone().unwrap_or_default()
    } else {
        format!(
            "{} {}",
            result.basic.first_name.clone().unwrap_or_default(),
            result.basic.last_name.clone().unwrap_or_default()
        )
        .trim()
        .to_string()
    };

    let location = result
        .addresses
        .iter()
        .find(|a| a.address_purpose.as_deref() == Some("LOCATION"))
        .or_else(|| result.addresses.first());

    let address_line = location.and_then(|a| a.address_1.clone()).filter(|s| !s.is_empty());
    let city_state_zip = location.map(|a| {
        format!(
            "{}, {} {}",
            a.city.clone().unwrap_or_default(),
            a.state.clone().unwrap_or_default(),
            a.postal_code.clone().unwrap_or_default()
        )
        .trim()
        .to_string()
    });
    let phone = location
        .and_then(|a| a.telephone_number.clone())
        .filter(|s| !s.is_empty());

    let primary_taxonomy = result
        .taxonomies
        .iter()
        .find(|t| t.primary)
        .or_else(|| result.taxonomies.first())
        .and_then(|t| t.desc.clone());

    let status_display = match result.basic.status.as_deref() {
        Some("A") => "Active".to_string(),
        Some(other) if !other.is_empty() => other.to_string(),
        _ => "Unknown".to_string(),
    };

    Ok(Some(NpiInfo {
        number: result.number,
        name,
        entity_type: if is_org { "Organization".to_string() } else { "Individual Provider".to_string() },
        credential: result.basic.credential.clone().filter(|s| !s.is_empty()),
        status_display,
        primary_taxonomy,
        address_line,
        city_state_zip,
        phone,
    }))
}
