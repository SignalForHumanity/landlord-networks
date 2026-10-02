//! Group county parcel records into landlord ownership networks.
//!
//! Parcels are linked when they share a normalized taxpayer mailing address
//! or a normalized entity owner name. Links are followed transitively with a
//! union-find, so LLC A and LLC C end up together when each shares something
//! with LLC B.

pub mod normalize;

use normalize::{OwnerName, normalize_address, normalize_owner, normalize_street};
use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};

/// Which CSV columns hold which field. Names match headers
/// case-insensitively, ignoring surrounding whitespace.
#[derive(Debug, Clone, Default)]
pub struct Columns {
    pub id: String,
    pub owners: Vec<String>,
    pub mail: Vec<String>,
    pub mail_city: Option<String>,
    pub mail_zip: Option<String>,
    pub site: Vec<String>,
}

/// One parcel, after merging all rows that share its ID.
#[derive(Debug, Clone)]
pub struct Parcel {
    pub id: String,
    pub owners_raw: Vec<String>,
    pub owners: Vec<OwnerName>,
    pub mail_raw: String,
    pub mail_key: Option<String>,
    pub site_raw: String,
    pub owner_occupied: bool,
}

fn find_column(headers: &[String], name: &str) -> Result<usize, String> {
    let want = name.trim().to_ascii_lowercase();
    headers
        .iter()
        .position(|h| h.trim().trim_start_matches('\u{feff}').to_ascii_lowercase() == want)
        .ok_or_else(|| {
            format!(
                "column '{name}' not found; available columns: {}",
                headers.join(", ")
            )
        })
}

/// Decode one CSV cell: UTF-8 when valid, otherwise Latin-1 (each byte is
/// one character), so distinct accented letters stay distinct.
fn decode(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => bytes.iter().map(|&b| b as char).collect(),
    }
}

/// Whether `needle` occurs in `haystack` on whole-token boundaries.
fn contains_tokens(haystack: &str, needle: &str) -> bool {
    format!(" {haystack} ").contains(&format!(" {needle} "))
}

fn join_cells(record: &[String], idx: &[usize]) -> String {
    idx.iter()
        .filter_map(|&i| record.get(i))
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Read parcels from CSV. Cells that are not valid UTF-8 (common in Latin-1
/// county exports) are decoded as Latin-1. Rows with an empty ID are skipped.
pub fn read_parcels<R: Read>(
    input: R,
    delimiter: u8,
    cols: &Columns,
    drop_unit: bool,
) -> Result<Vec<Parcel>, String> {
    let mut rdr = csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .flexible(true)
        .from_reader(input);
    let headers: Vec<String> = rdr
        .byte_headers()
        .map_err(|e| format!("cannot read CSV header: {e}"))?
        .iter()
        .map(decode)
        .collect();
    let id_i = find_column(&headers, &cols.id)?;
    let owner_i = cols
        .owners
        .iter()
        .map(|c| find_column(&headers, c))
        .collect::<Result<Vec<_>, _>>()?;
    let mail_i = cols
        .mail
        .iter()
        .map(|c| find_column(&headers, c))
        .collect::<Result<Vec<_>, _>>()?;
    let site_i = cols
        .site
        .iter()
        .map(|c| find_column(&headers, c))
        .collect::<Result<Vec<_>, _>>()?;
    let city_i = cols
        .mail_city
        .as_deref()
        .map(|c| find_column(&headers, c))
        .transpose()?;
    let zip_i = cols
        .mail_zip
        .as_deref()
        .map(|c| find_column(&headers, c))
        .transpose()?;

    let mut parcels: Vec<Parcel> = Vec::new();
    let mut by_id: HashMap<String, usize> = HashMap::new();
    for (line, rec) in rdr.byte_records().enumerate() {
        let rec = rec.map_err(|e| format!("CSV error near data row {}: {e}", line + 1))?;
        let rec: Vec<String> = rec.iter().map(decode).collect();
        let id = rec.get(id_i).map(|s| s.trim()).unwrap_or("").to_string();
        if id.is_empty() {
            continue;
        }
        let owners_raw: Vec<String> = owner_i
            .iter()
            .filter_map(|&i| rec.get(i))
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let mail_street = join_cells(&rec, &mail_i);
        let city = city_i.and_then(|i| rec.get(i)).map(String::as_str);
        let zip = zip_i.and_then(|i| rec.get(i)).map(String::as_str);
        let mail_key = normalize_address(&mail_street, city, zip, drop_unit);
        let mut mail_raw = mail_street.clone();
        for extra in [city, zip].into_iter().flatten() {
            let extra = extra.trim();
            if !extra.is_empty() {
                mail_raw.push(' ');
                mail_raw.push_str(extra);
            }
        }
        let site_raw = join_cells(&rec, &site_i);
        let site_street = normalize_street(&site_raw, true);
        let mail_cmp = normalize_street(&mail_street, true);
        let owner_occupied = !site_street.is_empty() && site_street == mail_cmp;

        let mut owners: Vec<OwnerName> = Vec::new();
        for raw in &owners_raw {
            let n = normalize_owner(raw);
            if !n.key.is_empty() {
                owners.push(n);
            } else if let Some(c) = n.care_of {
                // A "C/O ..." line in its own column belongs to the owner
                // named before it.
                if let Some(prev) = owners.last_mut().filter(|o| o.care_of.is_none()) {
                    prev.care_of = Some(c);
                }
            }
        }
        match by_id.get(&id) {
            Some(&p) => {
                let existing = &mut parcels[p];
                for raw in owners_raw {
                    if !existing.owners_raw.contains(&raw) {
                        existing.owners_raw.push(raw);
                    }
                }
                for norm in owners {
                    if !existing.owners.iter().any(|o| o.key == norm.key) {
                        existing.owners.push(norm);
                    }
                }
                if existing.mail_key.is_none() && mail_key.is_some() {
                    existing.mail_key = mail_key;
                    existing.mail_raw = mail_raw;
                }
                if existing.site_raw.is_empty() {
                    existing.site_raw = site_raw;
                }
                existing.owner_occupied |= owner_occupied;
            }
            None => {
                by_id.insert(id.clone(), parcels.len());
                parcels.push(Parcel {
                    id,
                    owners_raw,
                    owners,
                    mail_raw,
                    mail_key,
                    site_raw,
                    owner_occupied,
                });
            }
        }
    }
    Ok(parcels)
}

/// Which owner names may link parcels on their own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameLinking {
    /// Only business/trust/organization names (default). Namesake private
    /// individuals are never merged on name alone.
    Entities,
    /// Every owner name, including individuals.
    All,
    /// Never link on names; addresses only.
    None,
}

#[derive(Debug, Clone)]
pub struct LinkOptions {
    pub names: NameLinking,
    pub care_of: bool,
    pub ignore_addresses: HashSet<String>,
    pub max_names_per_address: Option<usize>,
    pub skip_owner_occupied: bool,
}

impl Default for LinkOptions {
    fn default() -> Self {
        LinkOptions {
            names: NameLinking::Entities,
            care_of: false,
            ignore_addresses: HashSet::new(),
            max_names_per_address: None,
            skip_owner_occupied: false,
        }
    }
}

/// A group of parcels believed to share an owner.
#[derive(Debug, Clone)]
pub struct Network {
    /// 1-based rank by parcel count.
    pub id: usize,
    /// Indices into the parcel slice, in input order.
    pub parcels: Vec<usize>,
    /// Distinct normalized owner names, most frequent first.
    pub owner_names: Vec<String>,
    /// Distinct normalized mailing addresses, most frequent first.
    pub addresses: Vec<String>,
    pub owner_occupied: usize,
}

impl Network {
    pub fn label(&self) -> String {
        format!("N{}", self.id)
    }
}

struct UnionFind {
    parent: Vec<usize>,
    rank: Vec<u8>,
}

impl UnionFind {
    fn new(n: usize) -> Self {
        UnionFind {
            parent: (0..n).collect(),
            rank: vec![0; n],
        }
    }
    fn find(&mut self, mut x: usize) -> usize {
        while self.parent[x] != x {
            self.parent[x] = self.parent[self.parent[x]];
            x = self.parent[x];
        }
        x
    }
    fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra == rb {
            return;
        }
        match self.rank[ra].cmp(&self.rank[rb]) {
            std::cmp::Ordering::Less => self.parent[ra] = rb,
            std::cmp::Ordering::Greater => self.parent[rb] = ra,
            std::cmp::Ordering::Equal => {
                self.parent[rb] = ra;
                self.rank[ra] += 1;
            }
        }
    }
}

/// Per-address statistics, used to spot generic addresses (tax services,
/// registered agents, government offices) that would over-merge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddressStat {
    pub address: String,
    pub parcels: usize,
    pub owner_names: usize,
}

/// Count parcels and distinct owner names per normalized mailing address,
/// sorted by distinct owner names, then parcels, descending.
pub fn address_stats(parcels: &[Parcel]) -> Vec<AddressStat> {
    let mut map: HashMap<&str, (usize, HashSet<&str>)> = HashMap::new();
    for p in parcels {
        if let Some(a) = &p.mail_key {
            let e = map.entry(a.as_str()).or_default();
            e.0 += 1;
            for o in &p.owners {
                e.1.insert(o.key.as_str());
            }
        }
    }
    let mut out: Vec<AddressStat> = map
        .into_iter()
        .map(|(a, (n, names))| AddressStat {
            address: a.to_string(),
            parcels: n,
            owner_names: names.len(),
        })
        .collect();
    out.sort_by(|a, b| {
        b.owner_names
            .cmp(&a.owner_names)
            .then(b.parcels.cmp(&a.parcels))
            .then(a.address.cmp(&b.address))
    });
    out
}

/// Addresses still used for linking that are shared by more than
/// `threshold` distinct owner names. These are usually tax or mortgage
/// servicers and registered agents, and they merge unrelated owners
/// (including private homeowners) into one network.
pub fn suspicious_addresses(
    parcels: &[Parcel],
    linked: &Linked,
    threshold: usize,
) -> Vec<AddressStat> {
    address_stats(parcels)
        .into_iter()
        .filter(|s| s.owner_names > threshold && s.parcels > 1)
        .filter(|s| linked.excluded_addresses.binary_search(&s.address).is_err())
        .collect()
}

fn ranked(counts: HashMap<String, usize>) -> Vec<String> {
    let mut v: Vec<(String, usize)> = counts.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    v.into_iter().map(|(k, _)| k).collect()
}

/// Result of linking: all networks (including single parcels) plus the
/// addresses that were excluded from linking.
#[derive(Debug, Clone)]
pub struct Linked {
    pub networks: Vec<Network>,
    /// Network index for each parcel (`None` when the parcel was skipped).
    pub network_of: Vec<Option<usize>>,
    pub excluded_addresses: Vec<String>,
}

/// Link parcels into networks. Networks are sorted by parcel count
/// (descending), then by first owner name, and numbered from 1.
pub fn build_networks(parcels: &[Parcel], opts: &LinkOptions) -> Linked {
    let mut excluded: HashSet<String> = opts.ignore_addresses.clone();
    if let Some(max) = opts.max_names_per_address {
        for s in address_stats(parcels) {
            if s.owner_names > max {
                excluded.insert(s.address);
            }
        }
    }
    let active: Vec<bool> = parcels
        .iter()
        .map(|p| !(opts.skip_owner_occupied && p.owner_occupied))
        .collect();

    let mut uf = UnionFind::new(parcels.len());
    let mut first_with_key: HashMap<String, usize> = HashMap::new();
    let mut link = |key: String, i: usize, uf: &mut UnionFind| match first_with_key.get(&key) {
        Some(&j) => uf.union(i, j),
        None => {
            first_with_key.insert(key, i);
        }
    };
    for (i, p) in parcels.iter().enumerate() {
        if !active[i] {
            continue;
        }
        if let Some(a) = &p.mail_key
            && !excluded.contains(a)
        {
            link(format!("A:{a}"), i, &mut uf);
        }
        for o in &p.owners {
            let use_name = match opts.names {
                NameLinking::All => true,
                NameLinking::Entities => o.is_entity,
                NameLinking::None => false,
            };
            if use_name {
                link(format!("N:{}", o.key), i, &mut uf);
            }
            // The care-of party follows the same name rule: a private
            // individual is only linked on name with `NameLinking::All`.
            if opts.care_of
                && let Some(c) = &o.care_of
                && (opts.names != NameLinking::Entities || normalize_owner(c).is_entity)
            {
                link(format!("N:{c}"), i, &mut uf);
            }
        }
    }

    let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
    for (i, &is_active) in active.iter().enumerate() {
        if is_active {
            groups.entry(uf.find(i)).or_default().push(i);
        }
    }
    let mut networks: Vec<Network> = groups
        .into_values()
        .map(|members| {
            let mut names: HashMap<String, usize> = HashMap::new();
            let mut addrs: HashMap<String, usize> = HashMap::new();
            let mut occ = 0;
            for &m in &members {
                let p = &parcels[m];
                for o in &p.owners {
                    *names.entry(o.key.clone()).or_default() += 1;
                }
                if let Some(a) = &p.mail_key {
                    *addrs.entry(a.clone()).or_default() += 1;
                }
                if p.owner_occupied {
                    occ += 1;
                }
            }
            Network {
                id: 0,
                parcels: members,
                owner_names: ranked(names),
                addresses: ranked(addrs),
                owner_occupied: occ,
            }
        })
        .collect();
    networks.sort_by(|a, b| {
        b.parcels
            .len()
            .cmp(&a.parcels.len())
            .then_with(|| a.owner_names.first().cmp(&b.owner_names.first()))
            .then_with(|| a.parcels.first().cmp(&b.parcels.first()))
    });
    let mut network_of = vec![None; parcels.len()];
    for (n, net) in networks.iter_mut().enumerate() {
        net.id = n + 1;
        for &m in &net.parcels {
            network_of[m] = Some(n);
        }
    }
    let mut excluded_addresses: Vec<String> = excluded.into_iter().collect();
    excluded_addresses.sort();
    Linked {
        networks,
        network_of,
        excluded_addresses,
    }
}

fn list(items: &[String], max: usize) -> String {
    if max == 0 || items.len() <= max {
        items.join("; ")
    } else {
        format!(
            "{}; ... (+{} more)",
            items[..max].join("; "),
            items.len() - max
        )
    }
}

fn csv_err(e: csv::Error) -> String {
    format!("write error: {e}")
}

/// Write one summary row per network with at least `min_size` parcels.
pub fn write_networks<W: Write>(
    out: W,
    networks: &[Network],
    min_size: usize,
    max_list: usize,
) -> Result<usize, String> {
    let mut w = csv::Writer::from_writer(out);
    w.write_record([
        "network_id",
        "parcels",
        "owner_name_count",
        "address_count",
        "owner_occupied_parcels",
        "owner_names",
        "mailing_addresses",
    ])
    .map_err(csv_err)?;
    let mut n = 0;
    for net in networks.iter().filter(|n| n.parcels.len() >= min_size) {
        w.write_record([
            net.label(),
            net.parcels.len().to_string(),
            net.owner_names.len().to_string(),
            net.addresses.len().to_string(),
            net.owner_occupied.to_string(),
            list(&net.owner_names, max_list),
            list(&net.addresses, max_list),
        ])
        .map_err(csv_err)?;
        n += 1;
    }
    w.flush().map_err(|e| format!("write error: {e}"))?;
    Ok(n)
}

/// Write one row per parcel that belongs to a network of at least
/// `min_size` parcels, grouped by network.
pub fn write_members<W: Write>(
    out: W,
    parcels: &[Parcel],
    networks: &[Network],
    min_size: usize,
) -> Result<(), String> {
    let mut w = csv::Writer::from_writer(out);
    w.write_record([
        "network_id",
        "parcel_id",
        "owner_names",
        "mailing_address",
        "site_address",
        "owner_occupied",
        "normalized_owner_names",
        "normalized_mailing_address",
    ])
    .map_err(csv_err)?;
    for net in networks.iter().filter(|n| n.parcels.len() >= min_size) {
        for &m in &net.parcels {
            let p = &parcels[m];
            let norm: Vec<String> = p.owners.iter().map(|o| o.key.clone()).collect();
            w.write_record([
                net.label(),
                p.id.clone(),
                p.owners_raw.join("; "),
                p.mail_raw.clone(),
                p.site_raw.clone(),
                if p.owner_occupied { "yes" } else { "no" }.to_string(),
                norm.join("; "),
                p.mail_key.clone().unwrap_or_default(),
            ])
            .map_err(csv_err)?;
        }
    }
    w.flush().map_err(|e| format!("write error: {e}"))?;
    Ok(())
}

/// Find parcels matching a query: exact parcel ID, or a case-insensitive
/// substring of an owner name, or whole words of a site address (so
/// "12 ELM ST" does not match "112 ELM ST").
pub fn find_parcels(parcels: &[Parcel], query: &str) -> Vec<usize> {
    let q = query.trim();
    if q.is_empty() {
        return Vec::new();
    }
    if let Some(i) = parcels.iter().position(|p| p.id.eq_ignore_ascii_case(q)) {
        return vec![i];
    }
    let ql = q.to_uppercase();
    let qn = normalize_owner(q).key;
    let qs = normalize_street(q, false);
    parcels
        .iter()
        .enumerate()
        .filter(|(_, p)| {
            p.owners_raw.iter().any(|o| o.to_uppercase().contains(&ql))
                || (!qn.is_empty() && p.owners.iter().any(|o| o.key.contains(&qn)))
                || (!qs.is_empty() && contains_tokens(&normalize_street(&p.site_raw, false), &qs))
        })
        .map(|(i, _)| i)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parcel(id: &str, owner: &str, mail: &str) -> Parcel {
        Parcel {
            id: id.into(),
            owners_raw: vec![owner.into()],
            owners: vec![normalize_owner(owner)],
            mail_raw: mail.into(),
            mail_key: normalize_address(mail, None, None, false),
            site_raw: String::new(),
            owner_occupied: false,
        }
    }

    fn groups(linked: &Linked, parcels: &[Parcel]) -> Vec<Vec<String>> {
        linked
            .networks
            .iter()
            .map(|n| {
                let mut ids: Vec<String> =
                    n.parcels.iter().map(|&i| parcels[i].id.clone()).collect();
                ids.sort();
                ids
            })
            .collect()
    }

    #[test]
    fn transitive_chain() {
        let ps = vec![
            parcel("1", "A LLC", "1 MAIN ST 60601"),
            parcel("2", "B LLC", "1 Main Street 60601"),
            parcel("3", "B L.L.C.", "PO BOX 9 60602"),
            parcel("4", "C LLC", "P.O. Box 9 60602"),
            parcel("5", "Unrelated Inc", "7 Oak Ave 60603"),
        ];
        let l = build_networks(&ps, &LinkOptions::default());
        assert_eq!(groups(&l, &ps), vec![vec!["1", "2", "3", "4"], vec!["5"]]);
        assert_eq!(l.networks[0].owner_names.len(), 3);
        assert_eq!(l.networks[0].addresses.len(), 2);
    }

    #[test]
    fn namesake_individuals_stay_apart_by_default() {
        let ps = vec![
            parcel("1", "SMITH JOHN", "1 Main St 60601"),
            parcel("2", "Smith, John", "99 Elm St 60609"),
        ];
        let l = build_networks(&ps, &LinkOptions::default());
        assert_eq!(l.networks.len(), 2);
        let opts = LinkOptions {
            names: NameLinking::All,
            ..Default::default()
        };
        assert_eq!(build_networks(&ps, &opts).networks.len(), 1);
    }

    #[test]
    fn ignore_and_max_names() {
        let ps = vec![
            parcel("1", "A LLC", "1 Tax Service Way 60601"),
            parcel("2", "B LLC", "1 Tax Service Way 60601"),
            parcel("3", "C LLC", "1 Tax Service Way 60601"),
        ];
        assert_eq!(
            build_networks(&ps, &LinkOptions::default()).networks.len(),
            1
        );
        let mut ignore = HashSet::new();
        ignore.insert("1 TAX SERVICE WAY 60601".to_string());
        let opts = LinkOptions {
            ignore_addresses: ignore,
            ..Default::default()
        };
        assert_eq!(build_networks(&ps, &opts).networks.len(), 3);
        let opts = LinkOptions {
            max_names_per_address: Some(2),
            ..Default::default()
        };
        let l = build_networks(&ps, &opts);
        assert_eq!(l.networks.len(), 3);
        assert_eq!(l.excluded_addresses, vec!["1 TAX SERVICE WAY 60601"]);
    }

    #[test]
    fn care_of_linking_is_opt_in() {
        let ps = vec![
            parcel("1", "A LLC C/O Big Mgmt", "1 Main St 60601"),
            parcel("2", "B LLC c/o BIG MANAGEMENT", "2 Main St 60601"),
        ];
        assert_eq!(
            build_networks(&ps, &LinkOptions::default()).networks.len(),
            2
        );
        let opts = LinkOptions {
            care_of: true,
            ..Default::default()
        };
        assert_eq!(build_networks(&ps, &opts).networks.len(), 1);
    }

    #[test]
    fn names_none_uses_addresses_only() {
        let ps = vec![
            parcel("1", "A LLC", "1 Main St 60601"),
            parcel("2", "A LLC", "2 Main St 60601"),
        ];
        let opts = LinkOptions {
            names: NameLinking::None,
            ..Default::default()
        };
        assert_eq!(build_networks(&ps, &opts).networks.len(), 2);
    }

    #[test]
    fn list_truncates() {
        let v: Vec<String> = ["a", "b", "c"].iter().map(|s| s.to_string()).collect();
        assert_eq!(list(&v, 2), "a; b; ... (+1 more)");
        assert_eq!(list(&v, 0), "a; b; c");
    }
}

#[cfg(test)]
mod review_tests {
    use super::*;

    fn cols(owners: &[&str]) -> Columns {
        Columns {
            id: "id".into(),
            owners: owners.iter().map(|s| s.to_string()).collect(),
            mail: vec!["mail".into()],
            mail_city: None,
            mail_zip: None,
            site: vec!["site".into()],
        }
    }

    #[test]
    fn lookup_address_does_not_match_inside_other_numbers() {
        let data =
            "id,owner,mail,site\n1,A LLC,1 X St 60601,12 Elm St\n2,B LLC,2 Y St 60601,112 Elm St\n";
        let ps = read_parcels(data.as_bytes(), b',', &cols(&["owner"]), false).unwrap();
        assert_eq!(find_parcels(&ps, "12 Elm Street"), vec![0]);
        assert_eq!(find_parcels(&ps, "Elm St"), vec![0, 1]);
    }

    #[test]
    fn latin1_letters_are_not_collapsed() {
        let mut data = b"id,owner,mail,site\n".to_vec();
        data.extend_from_slice(b"1,Mu\xf1oz LLC,1 X St 60601,\n2,Mu\xe1oz LLC,2 Y St 60602,\n");
        let ps = read_parcels(&data[..], b',', &cols(&["owner"]), false).unwrap();
        assert_ne!(ps[0].owners[0].key, ps[1].owners[0].key);
        assert_eq!(ps[0].owners[0].key, "MUÑOZ LLC");
        let l = build_networks(&ps, &LinkOptions::default());
        assert_eq!(l.networks.len(), 2);
    }

    #[test]
    fn care_of_in_its_own_column_is_kept() {
        let data = "id,o1,o2,mail,site\n1,A LLC,C/O Lakeside Mgmt,1 X St 60601,\n2,B LLC,c/o Lakeside Management,2 Y St 60602,\n";
        let ps = read_parcels(data.as_bytes(), b',', &cols(&["o1", "o2"]), false).unwrap();
        assert_eq!(ps[0].owners.len(), 1);
        assert_eq!(ps[0].owners[0].care_of.as_deref(), Some("LAKESIDE MGMT"));
        let opts = LinkOptions {
            care_of: true,
            ..Default::default()
        };
        assert_eq!(build_networks(&ps, &opts).networks.len(), 1);
    }

    #[test]
    fn care_of_namesake_individuals_respect_entities_mode() {
        let data = "id,owner,mail,site\n1,A LLC C/O Smith John,1 X St 60601,\n2,B LLC C/O John Smith,2 Y St 60602,\n3,C LLC C/O SMITH JOHN,3 Z St 60603,\n";
        let ps = read_parcels(data.as_bytes(), b',', &cols(&["owner"]), false).unwrap();
        let opts = LinkOptions {
            care_of: true,
            ..Default::default()
        };
        assert_eq!(build_networks(&ps, &opts).networks.len(), 3);
        let opts = LinkOptions {
            care_of: true,
            names: NameLinking::All,
            ..Default::default()
        };
        assert_eq!(build_networks(&ps, &opts).networks.len(), 2);
    }

    #[test]
    fn duplicate_rows_keep_raw_owner_names() {
        let data =
            "id,o1,o2,mail,site\n1,Acme LLC,,1 X St 60601,\n1,ET AL,Beta LLC,1 X St 60601,\n";
        let ps = read_parcels(data.as_bytes(), b',', &cols(&["o1", "o2"]), false).unwrap();
        assert_eq!(ps.len(), 1);
        assert!(ps[0].owners_raw.iter().any(|r| r == "Beta LLC"));
        assert_eq!(ps[0].owners.len(), 2);
    }

    #[test]
    fn suspicious_addresses_respect_exclusions() {
        let data = "id,owner,mail,site\n1,A LLC,9 Tax Way 75201,\n2,Smith Ann,9 Tax Way 75201,\n3,Lee Bo,9 Tax Way 75201,\n";
        let ps = read_parcels(data.as_bytes(), b',', &cols(&["owner"]), false).unwrap();
        let l = build_networks(&ps, &LinkOptions::default());
        let s = suspicious_addresses(&ps, &l, 2);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].address, "9 TAX WAY 75201");
        assert!(suspicious_addresses(&ps, &l, 3).is_empty());
        let opts = LinkOptions {
            max_names_per_address: Some(2),
            ..Default::default()
        };
        let l = build_networks(&ps, &opts);
        assert!(suspicious_addresses(&ps, &l, 2).is_empty());
    }

    #[test]
    fn namesake_trustees_stay_apart_by_default() {
        let data = "id,owner,mail,site\n1,SMITH JOHN TR,1 X St 60601,\n2,Smith John Trustee,2 Y St 60602,\n3,CHURCH JOHN,3 Z St 60603,\n4,Church John,4 W St 60604,\n";
        let ps = read_parcels(data.as_bytes(), b',', &cols(&["owner"]), false).unwrap();
        assert_eq!(
            build_networks(&ps, &LinkOptions::default()).networks.len(),
            4
        );
    }
}
