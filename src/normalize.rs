//! Normalization of owner names and mailing addresses found in US county
//! parcel (assessor) exports.
//!
//! All functions are pure and idempotent: normalizing an already-normalized
//! value returns it unchanged, so normalized keys printed by the tool can be
//! pasted back into an ignore list.

/// Result of normalizing one owner-name cell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerName {
    /// Normalized owner name, e.g. `ACME HOLDINGS LLC`.
    pub key: String,
    /// True when the name looks like a business, trust or other entity
    /// rather than a private individual.
    pub is_entity: bool,
    /// Normalized "care of" / "attention" party, if the cell contained one.
    pub care_of: Option<String>,
}

/// Words that mark a name as an organization, trust or other entity.
const ENTITY_WORDS: &[&str] = &[
    "LLC",
    "INC",
    "CORP",
    "CO",
    "LTD",
    "LP",
    "LLP",
    "LLLP",
    "PLLC",
    "PC",
    "TRUST",
    "ASSN",
    "ASSOC",
    "BANK",
    "HOLDINGS",
    "HOLDING",
    "PROPERTIES",
    "PROPERTY",
    "REALTY",
    "MGMT",
    "INVESTMENTS",
    "INVESTMENT",
    "INVESTORS",
    "PARTNERS",
    "PARTNERSHIP",
    "GROUP",
    "FUND",
    "VENTURES",
    "CAPITAL",
    "ENTERPRISES",
    "HOUSING",
    "APARTMENTS",
    "DEVELOPMENT",
    "ESTATES",
    "REIT",
    "AUTHORITY",
    "CHURCH",
    "MINISTRIES",
    "CITY",
    "COUNTY",
    "STATE",
    "FOUNDATION",
    "RENTALS",
    "HOMES",
    "SERIES",
];

/// Multi-word or spelled-out entity forms mapped to one canonical token.
/// Applied to token sequences, longest first.
const NAME_PHRASES: &[(&[&str], &str)] = &[
    (&["LIMITED", "LIABILITY", "COMPANY"], "LLC"),
    (&["LIMITED", "LIABILITY", "CO"], "LLC"),
    (&["LIMITED", "PARTNERSHIP"], "LP"),
    (&["L", "L", "C"], "LLC"),
    (&["L", "L", "P"], "LLP"),
    (&["L", "P"], "LP"),
    (&["ET", "AL"], ""),
    (&["ET", "UX"], ""),
];

/// Single-word entity spellings mapped to canonical tokens.
const NAME_WORDS: &[(&str, &str)] = &[
    ("INCORPORATED", "INC"),
    ("CORPORATION", "CORP"),
    ("COMPANY", "CO"),
    ("LIMITED", "LTD"),
    ("ASSOCIATION", "ASSN"),
    ("ASSOCIATES", "ASSOC"),
    ("MANAGEMENT", "MGMT"),
    ("TRUSTEE", "TR"),
    ("TRUSTEES", "TR"),
    ("TRSTE", "TR"),
    ("TRS", "TR"),
    ("TRST", "TRUST"),
    ("ETAL", ""),
    ("ETUX", ""),
];

/// Address words mapped to USPS standard abbreviations (Publication 28).
const ADDRESS_WORDS: &[(&str, &str)] = &[
    ("STREET", "ST"),
    ("STR", "ST"),
    ("AVENUE", "AVE"),
    ("AV", "AVE"),
    ("AVN", "AVE"),
    ("ROAD", "RD"),
    ("DRIVE", "DR"),
    ("BOULEVARD", "BLVD"),
    ("BLV", "BLVD"),
    ("LANE", "LN"),
    ("COURT", "CT"),
    ("PLACE", "PL"),
    ("TERRACE", "TER"),
    ("PARKWAY", "PKWY"),
    ("PKY", "PKWY"),
    ("HIGHWAY", "HWY"),
    ("CIRCLE", "CIR"),
    ("SQUARE", "SQ"),
    ("TRAIL", "TRL"),
    ("PLAZA", "PLZ"),
    ("CENTER", "CTR"),
    ("CENTRE", "CTR"),
    ("EXPRESSWAY", "EXPY"),
    ("FREEWAY", "FWY"),
    ("TURNPIKE", "TPKE"),
    ("ALLEY", "ALY"),
    ("CROSSING", "XING"),
    ("POINT", "PT"),
    ("NORTH", "N"),
    ("SOUTH", "S"),
    ("EAST", "E"),
    ("WEST", "W"),
    ("NORTHEAST", "NE"),
    ("NORTHWEST", "NW"),
    ("SOUTHEAST", "SE"),
    ("SOUTHWEST", "SW"),
    ("FIRST", "1ST"),
    ("SECOND", "2ND"),
    ("THIRD", "3RD"),
    ("FOURTH", "4TH"),
    ("FIFTH", "5TH"),
    ("SIXTH", "6TH"),
    ("SEVENTH", "7TH"),
    ("EIGHTH", "8TH"),
    ("NINTH", "9TH"),
    ("TENTH", "10TH"),
    // Unit designators all collapse to "#" so "STE 200" == "UNIT 200".
    ("SUITE", "#"),
    ("STE", "#"),
    ("APARTMENT", "#"),
    ("APT", "#"),
    ("UNIT", "#"),
    ("ROOM", "#"),
    ("RM", "#"),
    ("FLOOR", "#"),
    ("FL", "#"),
    ("NO", "#"),
];

/// Values that mean "no usable address".
const PLACEHOLDER_ADDRESSES: &[&str] = &[
    "",
    "UNKNOWN",
    "NONE",
    "NA",
    "N A",
    "SAME",
    "NO ADDRESS",
    "ADDRESS UNKNOWN",
    "UNKNOWN ADDRESS",
    "NOT AVAILABLE",
];

/// Uppercase, drop `.`, `,` and apostrophes (so `L.L.C.` becomes `LLC`),
/// turn `&` into `AND`, and every other non-alphanumeric character (except
/// `#` and `/` when `keep` contains them) into a space. Returns tokens.
fn tokenize(raw: &str, keep: &[char]) -> Vec<String> {
    let mut cleaned = String::with_capacity(raw.len() + 8);
    for ch in raw.chars() {
        let up = ch.to_ascii_uppercase();
        match up {
            '.' | ',' | '\'' | '`' => {}
            '&' => cleaned.push_str(" AND "),
            c if c.is_ascii_alphanumeric() => cleaned.push(c),
            c if keep.contains(&c) => {
                cleaned.push(' ');
                cleaned.push(c);
                cleaned.push(' ');
            }
            c if c.is_alphanumeric() => cleaned.extend(c.to_uppercase()),
            _ => cleaned.push(' '),
        }
    }
    cleaned.split_whitespace().map(str::to_string).collect()
}

fn lookup<'a>(table: &'a [(&str, &str)], word: &str) -> Option<&'a str> {
    table
        .iter()
        .find(|(from, _)| *from == word)
        .map(|(_, to)| *to)
}

/// Replace phrases (token sequences) and single words using the name tables.
fn canonical_name_tokens(tokens: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(tokens.len());
    let mut i = 0;
    'outer: while i < tokens.len() {
        for (phrase, replacement) in NAME_PHRASES {
            let n = phrase.len();
            if i + n <= tokens.len()
                && tokens[i..i + n]
                    .iter()
                    .zip(phrase.iter())
                    .all(|(a, b)| a == b)
            {
                if !replacement.is_empty() {
                    out.push((*replacement).to_string());
                }
                i += n;
                continue 'outer;
            }
        }
        let t = &tokens[i];
        match lookup(NAME_WORDS, t) {
            Some("") => {}
            Some(r) => out.push(r.to_string()),
            None => out.push(t.clone()),
        }
        i += 1;
    }
    if out.first().map(String::as_str) == Some("THE") && out.len() > 1 {
        out.remove(0);
    }
    out
}

/// Normalize one owner-name cell.
///
/// The text after `C/O`, `ATTN` or `%` is split off into
/// [`OwnerName::care_of`] because it names an agent or manager, not the
/// owner.
pub fn normalize_owner(raw: &str) -> OwnerName {
    let tokens = tokenize(raw, &['/', '%']);
    let mut split_at = None;
    let mut skip = 0;
    for i in 0..tokens.len() {
        let t = tokens[i].as_str();
        if t == "%" || t == "ATTN" {
            split_at = Some(i);
            skip = 1;
            break;
        }
        if t == "C"
            && tokens.get(i + 1).map(String::as_str) == Some("/")
            && tokens.get(i + 2).map(String::as_str) == Some("O")
        {
            split_at = Some(i);
            skip = 3;
            break;
        }
    }
    let (owner_tokens, care_tokens): (Vec<String>, Vec<String>) = match split_at {
        Some(i) => (
            tokens[..i].to_vec(),
            tokens[(i + skip).min(tokens.len())..].to_vec(),
        ),
        None => (tokens, Vec::new()),
    };
    let clean = |v: &[String]| -> Vec<String> {
        let v: Vec<String> = v
            .iter()
            .filter(|t| *t != "/" && *t != "%")
            .cloned()
            .collect();
        canonical_name_tokens(&v)
    };
    let owner = clean(&owner_tokens);
    let care = clean(&care_tokens);
    let is_entity = looks_like_entity(&owner);
    OwnerName {
        key: owner.join(" "),
        is_entity,
        care_of: if care.is_empty() {
            None
        } else {
            Some(care.join(" "))
        },
    }
}

/// True when normalized name tokens mark a business, trust or organization.
///
/// `TR` alone is not enough: it usually means an individual acting as
/// trustee ("SMITH JOHN TR"), and namesake individuals must not be merged.
/// `CO` before `TR` is "co-trustee", not "company". `CHURCH` as the first
/// word (and not "CHURCH OF ...") is the surname in "CHURCH JOHN".
fn looks_like_entity(tokens: &[String]) -> bool {
    tokens.iter().enumerate().any(|(i, t)| {
        let next = tokens.get(i + 1).map(String::as_str);
        match t.as_str() {
            "CO" if next == Some("TR") => false,
            "CHURCH" if i == 0 && next != Some("OF") => false,
            w => ENTITY_WORDS.contains(&w),
        }
    })
}

fn is_digits(s: &str, n: usize) -> bool {
    s.len() == n && s.bytes().all(|b| b.is_ascii_digit())
}

/// Normalize street-address tokens: USPS abbreviations, PO Box forms and
/// unit designators. Optionally drops the unit (`# 200`).
fn canonical_address_tokens(tokens: &[String], drop_unit: bool) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(tokens.len());
    let mut i = 0;
    while i < tokens.len() {
        let t = tokens[i].as_str();
        let next = tokens.get(i + 1).map(String::as_str);
        let next2 = tokens.get(i + 2).map(String::as_str);
        // PO Box variants.
        if t == "P" && next == Some("O") && matches!(next2, Some("BOX") | Some("BX")) {
            out.extend(["PO".to_string(), "BOX".to_string()]);
            i += 3;
            continue;
        }
        if t == "POST" && next == Some("OFFICE") && matches!(next2, Some("BOX") | Some("BX")) {
            out.extend(["PO".to_string(), "BOX".to_string()]);
            i += 3;
            continue;
        }
        if (t == "PO" || t == "POB") && matches!(next, Some("BOX") | Some("BX")) {
            out.extend(["PO".to_string(), "BOX".to_string()]);
            i += 2;
            continue;
        }
        if t == "POB" || t == "POBOX" {
            out.extend(["PO".to_string(), "BOX".to_string()]);
            i += 1;
            continue;
        }
        // "NO" is only a unit designator when followed by a number.
        if t == "NO" && !next.is_some_and(|n| n.bytes().any(|b| b.is_ascii_digit())) {
            out.push(t.to_string());
            i += 1;
            continue;
        }
        // "FL" before a ZIP (or at the end) is Florida, not "floor".
        if t == "FL" && next.is_none_or(|n| is_digits(n, 5)) {
            out.push(t.to_string());
            i += 1;
            continue;
        }
        let mapped = lookup(ADDRESS_WORDS, t).unwrap_or(t);
        // Collapse "# #" produced by e.g. "STE #200".
        if mapped == "#" && out.last().map(String::as_str) == Some("#") {
            i += 1;
            continue;
        }
        out.push(mapped.to_string());
        i += 1;
    }
    if drop_unit {
        let mut kept = Vec::with_capacity(out.len());
        let mut j = 0;
        while j < out.len() {
            if out[j] == "#" {
                j += 2;
                continue;
            }
            kept.push(out[j].clone());
            j += 1;
        }
        out = kept;
    }
    out
}

/// Normalize a street address only (no city or ZIP handling). Used to
/// compare a mailing address with the parcel's own site address.
pub fn normalize_street(raw: &str, drop_unit: bool) -> String {
    canonical_address_tokens(&tokenize(raw, &['#']), drop_unit).join(" ")
}

/// Normalize a full mailing address into a matching key.
///
/// `street` may already contain city/state/ZIP (single-column exports); a
/// trailing ZIP+4 suffix is reduced to ZIP5. When `zip` is given it is
/// appended (as ZIP5), otherwise `city` is appended when given. Returns
/// `None` for blank or placeholder addresses.
pub fn normalize_address(
    street: &str,
    city: Option<&str>,
    zip: Option<&str>,
    drop_unit: bool,
) -> Option<String> {
    let mut tokens = canonical_address_tokens(&tokenize(street, &['#']), drop_unit);
    // Drop ZIP+4 extension: "60601 1234" at the end.
    if tokens.len() >= 2 {
        let n = tokens.len();
        if is_digits(&tokens[n - 1], 4) && is_digits(&tokens[n - 2], 5) {
            tokens.pop();
        }
    }
    let joined = tokens.join(" ");
    if PLACEHOLDER_ADDRESSES.contains(&joined.as_str()) {
        return None;
    }
    let zip5: Option<String> = zip.and_then(|z| {
        let digits: String = z.chars().filter(char::is_ascii_digit).take(5).collect();
        (digits.len() == 5).then_some(digits)
    });
    if let Some(z) = zip5 {
        if tokens.last() != Some(&z) {
            tokens.push(z);
        }
    } else if let Some(c) = city {
        let ct = tokenize(c, &[]);
        let ct = canonical_address_tokens(&ct, false);
        if !ct.is_empty() && !tokens.ends_with(&ct) {
            tokens.extend(ct);
        }
    }
    Some(tokens.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entity_suffixes_are_canonical() {
        for raw in [
            "Acme Holdings, L.L.C.",
            "ACME HOLDINGS LLC",
            "acme holdings limited liability company",
            "The Acme Holdings L L C",
        ] {
            let n = normalize_owner(raw);
            assert_eq!(n.key, "ACME HOLDINGS LLC", "{raw}");
            assert!(n.is_entity);
        }
        assert_eq!(
            normalize_owner("Smith & Sons Incorporated").key,
            "SMITH AND SONS INC"
        );
        assert_eq!(normalize_owner("Big Corporation").key, "BIG CORP");
    }

    #[test]
    fn individuals_are_not_entities() {
        let n = normalize_owner("SMITH, JOHN A ET AL");
        assert_eq!(n.key, "SMITH JOHN A");
        assert!(!n.is_entity);
    }

    #[test]
    fn trusts_are_entities() {
        let n = normalize_owner("Jones Family Trust, Trustee");
        assert_eq!(n.key, "JONES FAMILY TRUST TR");
        assert!(n.is_entity);
    }

    #[test]
    fn care_of_is_split_off() {
        let n = normalize_owner("123 Main LLC C/O Pangea Management Co");
        assert_eq!(n.key, "123 MAIN LLC");
        assert_eq!(n.care_of.as_deref(), Some("PANGEA MGMT CO"));
        let n = normalize_owner("Elm Street LP % Big Mgmt Inc");
        assert_eq!(n.key, "ELM STREET LP");
        assert_eq!(n.care_of.as_deref(), Some("BIG MGMT INC"));
        let n = normalize_owner("Oak LLC ATTN: Legal Dept");
        assert_eq!(n.key, "OAK LLC");
        assert_eq!(n.care_of.as_deref(), Some("LEGAL DEPT"));
    }

    #[test]
    fn address_abbreviations() {
        let a = normalize_address(
            "123 North Main Street, Suite 200",
            None,
            Some("60601-1234"),
            false,
        );
        let b = normalize_address("123 N MAIN ST STE #200", None, Some("60601"), false);
        let c = normalize_address("123 N. Main St. Unit 200 60601", None, None, false);
        assert_eq!(a.as_deref(), Some("123 N MAIN ST # 200 60601"));
        assert_eq!(a, b);
        assert_eq!(a, c);
    }

    #[test]
    fn po_box_forms() {
        let want = Some("PO BOX 77 60601".to_string());
        for raw in [
            "P.O. Box 77",
            "PO BOX 77",
            "Post Office Box 77",
            "POB 77",
            "P O Bx 77",
        ] {
            assert_eq!(
                normalize_address(raw, None, Some("60601"), false),
                want,
                "{raw}"
            );
        }
    }

    #[test]
    fn zip_plus_four_in_single_column() {
        assert_eq!(
            normalize_address("9 Elm Ave Springfield IL 62701-0001", None, None, false).as_deref(),
            Some("9 ELM AVE SPRINGFIELD IL 62701")
        );
    }

    #[test]
    fn city_used_when_no_zip() {
        assert_eq!(
            normalize_address("9 Elm Ave", Some("Springfield"), None, false).as_deref(),
            Some("9 ELM AVE SPRINGFIELD")
        );
    }

    #[test]
    fn drop_unit_option() {
        assert_eq!(
            normalize_address("500 Lake Shore Dr Ste 12", None, Some("60611"), true).as_deref(),
            Some("500 LAKE SHORE DR 60611")
        );
    }

    #[test]
    fn placeholders_are_none() {
        for raw in ["", "  ", "UNKNOWN", "n/a", "Same", "No Address"] {
            assert_eq!(normalize_address(raw, None, None, false), None, "{raw:?}");
        }
    }

    #[test]
    fn no_without_number_is_kept() {
        assert_eq!(normalize_street("1 No Name Rd", false), "1 NO NAME RD");
        assert_eq!(normalize_street("1 Main St No 5", false), "1 MAIN ST # 5");
    }

    #[test]
    fn normalization_is_idempotent() {
        let a = normalize_address(
            "123 North Main Street, Suite 200",
            None,
            Some("60601"),
            false,
        )
        .unwrap();
        assert_eq!(normalize_address(&a, None, None, false).unwrap(), a);
        let n = normalize_owner("The Acme Holdings, L.L.C.").key;
        assert_eq!(normalize_owner(&n).key, n);
    }
}

#[cfg(test)]
mod review_tests {
    use super::*;

    #[test]
    fn florida_is_not_a_floor() {
        assert_eq!(
            normalize_address("1 Main St Miami FL 33101", None, None, true).as_deref(),
            Some("1 MAIN ST MIAMI FL 33101")
        );
        assert_eq!(
            normalize_address("1 Main St Miami FL 33101-1234", None, None, false).as_deref(),
            Some("1 MAIN ST MIAMI FL 33101")
        );
        assert_eq!(
            normalize_address("1 Main St FL", None, Some("33101"), false).as_deref(),
            Some("1 MAIN ST FL 33101")
        );
        assert_eq!(normalize_street("1 Main St Fl 2", false), "1 MAIN ST # 2");
    }

    #[test]
    fn individual_trustees_and_surnames_are_not_entities() {
        for raw in [
            "SMITH JOHN TR",
            "Smith, John, Trustee",
            "Smith John Co-Trustee",
            "Church, John",
        ] {
            assert!(!normalize_owner(raw).is_entity, "{raw}");
        }
        for raw in [
            "Jones Family Trust",
            "Chicago Title Land Trust Co Tr 1234",
            "First Baptist Church",
            "Church of God in Christ",
            "Acme Co",
        ] {
            assert!(normalize_owner(raw).is_entity, "{raw}");
        }
    }
}
