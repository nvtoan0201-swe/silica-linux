use serde::{Deserialize, Serialize};

use super::grammar::TokenKind;

/// A syntax highlighting color scheme, chosen in Settings. Each theme has a light and
/// a dark palette, picked by the window's appearance.
///
/// Only token colors change: plain text stays the label color and the editor keeps its
/// glass card, so comment grays meant for an opaque, near-black background are lifted a
/// little in the dark palettes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SyntaxTheme {
    #[default]
    One,
    Github,
    Xcode,
    Solarized,
    Catppuccin,
    Gruvbox,
    TokyoNight,
    RosePine,
    Dracula,
}

struct Palette {
    keyword: u32,
    ty: u32,
    string: u32,
    number: u32,
    comment: u32,
    function: u32,
    /// Properties, tags, variables and Markdown headings.
    variable: u32,
    link: u32,
    inserted: u32,
    deleted: u32,
    attribute: Option<u32>,
    preprocessor: Option<u32>,
}

impl Palette {
    fn get(&self, kind: TokenKind) -> u32 {
        match kind {
            TokenKind::Keyword => self.keyword,
            TokenKind::Preprocessor => self.preprocessor.unwrap_or(self.keyword),
            TokenKind::Type => self.ty,
            TokenKind::String => self.string,
            TokenKind::Number => self.number,
            TokenKind::Attribute => self.attribute.unwrap_or(self.number),
            TokenKind::Comment => self.comment,
            TokenKind::Function => self.function,
            TokenKind::Property | TokenKind::Tag | TokenKind::Variable | TokenKind::Heading => {
                self.variable
            }
            TokenKind::Link => self.link,
            TokenKind::Inserted => self.inserted,
            TokenKind::Deleted => self.deleted,
        }
    }
}

#[allow(clippy::too_many_arguments)]
const fn p(
    keyword: u32,
    ty: u32,
    string: u32,
    number: u32,
    comment: u32,
    function: u32,
    variable: u32,
    link: u32,
    inserted: u32,
    deleted: u32,
) -> Palette {
    Palette {
        keyword,
        ty,
        string,
        number,
        comment,
        function,
        variable,
        link,
        inserted,
        deleted,
        attribute: None,
        preprocessor: None,
    }
}

const fn with(mut palette: Palette, attribute: Option<u32>, preprocessor: Option<u32>) -> Palette {
    palette.attribute = attribute;
    palette.preprocessor = preprocessor;
    palette
}

// Atom's One Light / One Dark. One Dark's comment gray (5C6370) nearly vanishes on the
// glass card, so dark mode uses a lighter shade of it.
const ONE: (Palette, Palette) = (
    p(0xA626A4, 0xC18401, 0x50A14F, 0x986801, 0xA0A1A7, 0x4078F2, 0xE45649, 0x0184BC, 0x50A14F, 0xE45649),
    p(0xC678DD, 0xE5C07B, 0x98C379, 0xD19A66, 0x9DA5B3, 0x61AFEF, 0xE06C75, 0x56B6C2, 0x98C379, 0xE06C75),
);
// GitHub Light / GitHub Dark (Primer).
const GITHUB: (Palette, Palette) = (
    p(0xCF222E, 0x953800, 0x0A3069, 0x0550AE, 0x6E7781, 0x8250DF, 0x116329, 0x0969DA, 0x1A7F37, 0xCF222E),
    p(0xFF7B72, 0xFFA657, 0xA5D6FF, 0x79C0FF, 0x8B949E, 0xD2A8FF, 0x7EE787, 0x58A6FF, 0x3FB950, 0xF85149),
);
// Xcode's Standard preset, sampled from the editor.
const XCODE: (Palette, Palette) = (
    with(
        p(0xAC3261, 0x853CC1, 0xB33421, 0x286DAB, 0x606578, 0x265C52, 0x2C7290, 0x1146B3, 0x248A3D, 0xD70015),
        Some(0xAC3261),
        Some(0x805F24),
    ),
    with(
        p(0xED80A8, 0xC793F7, 0xEF8772, 0xDE9F2E, 0xA2AAC4, 0x479F92, 0x4CC1F5, 0x5B8EDD, 0x30D158, 0xFF453A),
        Some(0xED80A8),
        Some(0xDE9F2E),
    ),
);
// Solarized's accents are shared by both modes; only the comment tone differs.
const SOLARIZED: (Palette, Palette) = (
    with(
        p(0x859900, 0xB58900, 0x2AA198, 0xD33682, 0x93A1A1, 0x268BD2, 0xCB4B16, 0x6C71C4, 0x859900, 0xDC322F),
        None,
        Some(0xCB4B16),
    ),
    with(
        p(0x859900, 0xB58900, 0x2AA198, 0xD33682, 0x839496, 0x268BD2, 0xCB4B16, 0x6C71C4, 0x859900, 0xDC322F),
        None,
        Some(0xCB4B16),
    ),
);
// Catppuccin Latte / Mocha.
const CATPPUCCIN: (Palette, Palette) = (
    with(
        p(0x8839EF, 0xDF8E1D, 0x40A02B, 0xFE640B, 0x7C7F93, 0x1E66F5, 0xE64553, 0x209FB5, 0x40A02B, 0xD20F39),
        Some(0xDF8E1D),
        Some(0xEA76CB),
    ),
    with(
        p(0xCBA6F7, 0xF9E2AF, 0xA6E3A1, 0xFAB387, 0x9399B2, 0x89B4FA, 0xEBA0AC, 0x74C7EC, 0xA6E3A1, 0xF38BA8),
        Some(0xF9E2AF),
        Some(0xF5C2E7),
    ),
);
// Gruvbox Light / Dark.
const GRUVBOX: (Palette, Palette) = (
    with(
        p(0x9D0006, 0xB57614, 0x79740E, 0x8F3F71, 0x928374, 0x427B58, 0x076678, 0x076678, 0x79740E, 0x9D0006),
        Some(0xAF3A03),
        Some(0xAF3A03),
    ),
    with(
        p(0xFB4934, 0xFABD2F, 0xB8BB26, 0xD3869B, 0xA89984, 0x8EC07C, 0x83A598, 0x83A598, 0xB8BB26, 0xFB4934),
        Some(0xFE8019),
        Some(0xFE8019),
    ),
);
// Tokyo Night Day / Night. Night's comment (565F89) is lifted for the glass.
const TOKYO_NIGHT: (Palette, Palette) = (
    with(
        p(0x9854F1, 0x007197, 0x587539, 0xB15C00, 0x848CB5, 0x2E7DE9, 0xF52A65, 0x007197, 0x587539, 0xF52A65),
        Some(0x8C6C3E),
        None,
    ),
    with(
        p(0xBB9AF7, 0x2AC3DE, 0x9ECE6A, 0xFF9E64, 0x8089B3, 0x7AA2F7, 0xF7768E, 0x7DCFFF, 0x9ECE6A, 0xF7768E),
        Some(0xE0AF68),
        None,
    ),
);
// Rosé Pine Dawn / Rosé Pine (with Moon's pine, which reads on the glass card).
const ROSE_PINE: (Palette, Palette) = (
    p(0x286983, 0x56949F, 0xEA9D34, 0xB4637A, 0x9893A5, 0xD7827E, 0x907AA9, 0x56949F, 0x286983, 0xB4637A),
    p(0x3E8FB0, 0x9CCFD8, 0xF6C177, 0xEB6F92, 0x908CAA, 0xEBBCBA, 0xC4A7E7, 0x9CCFD8, 0x3E8FB0, 0xEB6F92),
);
// Alucard (Dracula's official light theme) / Dracula, with Dracula's comment lifted.
const DRACULA: (Palette, Palette) = (
    p(0xA3144D, 0x036A96, 0x846E15, 0x644AC9, 0x6C664B, 0x14710A, 0xA34D14, 0x036A96, 0x14710A, 0xCB3A2A),
    p(0xFF79C6, 0x8BE9FD, 0xF1FA8C, 0xBD93F9, 0x8A96C4, 0x50FA7B, 0xFFB86C, 0x8BE9FD, 0x50FA7B, 0xFF5555),
);

impl SyntaxTheme {
    pub const ALL: [SyntaxTheme; 9] = [
        SyntaxTheme::One,
        SyntaxTheme::Github,
        SyntaxTheme::Xcode,
        SyntaxTheme::Solarized,
        SyntaxTheme::Catppuccin,
        SyntaxTheme::Gruvbox,
        SyntaxTheme::TokyoNight,
        SyntaxTheme::RosePine,
        SyntaxTheme::Dracula,
    ];

    pub fn name(self) -> &'static str {
        match self {
            SyntaxTheme::One => "Atom One",
            SyntaxTheme::Github => "GitHub",
            SyntaxTheme::Xcode => "Xcode",
            SyntaxTheme::Solarized => "Solarized",
            SyntaxTheme::Catppuccin => "Catppuccin",
            SyntaxTheme::Gruvbox => "Gruvbox",
            SyntaxTheme::TokyoNight => "Tokyo Night",
            SyntaxTheme::RosePine => "Rosé Pine",
            SyntaxTheme::Dracula => "Dracula",
        }
    }

    fn palettes(self) -> &'static (Palette, Palette) {
        match self {
            SyntaxTheme::One => &ONE,
            SyntaxTheme::Github => &GITHUB,
            SyntaxTheme::Xcode => &XCODE,
            SyntaxTheme::Solarized => &SOLARIZED,
            SyntaxTheme::Catppuccin => &CATPPUCCIN,
            SyntaxTheme::Gruvbox => &GRUVBOX,
            SyntaxTheme::TokyoNight => &TOKYO_NIGHT,
            SyntaxTheme::RosePine => &ROSE_PINE,
            SyntaxTheme::Dracula => &DRACULA,
        }
    }

    /// The token's color as 0xRRGGBB.
    pub fn color(self, kind: TokenKind, dark: bool) -> u32 {
        let (light, dark_palette) = self.palettes();
        if dark { dark_palette.get(kind) } else { light.get(kind) }
    }

    /// Link color in the Markdown preview.
    pub fn link(self, dark: bool) -> u32 {
        let (light, dark_palette) = self.palettes();
        if dark { dark_palette.link } else { light.link }
    }
}
