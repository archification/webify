use ratatui::prelude::*;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
#[allow(dead_code)]
pub enum SolarizedDark {
    Base03,
    Base02,
    Base01,
    Base00,
    Base0,
    Base1,
    Base2,
    Base3,
    Yellow,
    Orange,
    Red,
    Magenta,
    Violet,
    Blue,
    Cyan,
    Green,
}

impl SolarizedDark {
    pub const fn color(&self) -> Color {
        match self {
            SolarizedDark::Base03 => Color::from_u32(0x002b36),
            SolarizedDark::Base02 => Color::from_u32(0x073642),
            SolarizedDark::Base01 => Color::from_u32(0x586e75),
            SolarizedDark::Base00 => Color::from_u32(0x657b83),
            SolarizedDark::Base0  => Color::from_u32(0x839496),
            SolarizedDark::Base1  => Color::from_u32(0x93a1a1),
            SolarizedDark::Base2  => Color::from_u32(0xeee8d5),
            SolarizedDark::Base3  => Color::from_u32(0xfdf6e3),
            SolarizedDark::Yellow => Color::from_u32(0xb58900),
            SolarizedDark::Orange => Color::from_u32(0xcb4b16),
            SolarizedDark::Red    => Color::from_u32(0xdc322f),
            SolarizedDark::Magenta=> Color::from_u32(0xd33682),
            SolarizedDark::Violet => Color::from_u32(0x6c71c4),
            SolarizedDark::Blue   => Color::from_u32(0x268bd2),
            SolarizedDark::Cyan   => Color::from_u32(0x2aa198),
            SolarizedDark::Green  => Color::from_u32(0x859900),
        }
    }
}

impl From<SolarizedDark> for Color {
    fn from(val: SolarizedDark) -> Color {
        val.color()
    }
}
