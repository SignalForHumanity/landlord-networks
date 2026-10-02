use std::path::PathBuf;
use std::process::{Command, Output};

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/parcels.csv")
}

fn run(sub: &str, extra: &[&str]) -> Output {
    let f = fixture();
    let mut args: Vec<&str> = vec![
        sub,
        f.to_str().unwrap(),
        "--id",
        "PIN",
        "--owner",
        "OWNER1",
        "--owner",
        "owner2",
        "--mail",
        "MAIL_ADDR",
        "--mail-city",
        "MAIL_CITY",
        "--mail-zip",
        "MAIL_ZIP",
        "--site",
        "SITE_ADDR",
    ];
    args.extend_from_slice(extra);
    Command::new(env!("CARGO_BIN_EXE_landlord-networks"))
        .args(&args)
        .output()
        .expect("run binary")
}

fn stdout(o: &Output) -> String {
    assert!(
        o.status.success(),
        "failed: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8(o.stdout.clone()).unwrap()
}

/// Parse networks CSV into (parcels, owner_names) per row.
fn rows(csv_text: &str) -> Vec<Vec<String>> {
    let mut r = csv::Reader::from_reader(csv_text.as_bytes());
    r.records()
        .map(|rec| rec.unwrap().iter().map(str::to_string).collect())
        .collect()
}

#[test]
fn default_networks() {
    let out = stdout(&run("networks", &[]));
    let rows = rows(&out);
    // Chain of 4 (address -> name -> PO box), tax-service address of 3, Garcia pair.
    let sizes: Vec<&str> = rows.iter().map(|r| r[1].as_str()).collect();
    assert_eq!(sizes, vec!["4", "3", "2"]);
    assert_eq!(rows[0][0], "N1");
    assert!(rows[0][5].contains("LOGAN SQ HOLDINGS LLC"));
    assert!(rows[0][5].contains("FULLERTON PARTNERS LP"));
    assert!(rows[0][6].contains("350 W HUBBARD ST # 300 60654"));
    assert!(rows[0][6].contains("PO BOX 4410 60680"));
    assert!(rows[2][5].contains("GARCIA LUIS"));
    // Namesake individuals SMITH JOHN are not merged: not in any network >= 2.
    assert!(!out.contains("SMITH JOHN"));
}

#[test]
fn min_size_one_includes_singletons() {
    let out = stdout(&run("networks", &["--min-size", "1"]));
    let rows = rows(&out);
    // 4 + 3 + 2 + SMITH(1) + SMITH(1) + Kedzie(1) = 6 networks; blank PIN skipped,
    // duplicate PIN merged.
    assert_eq!(rows.len(), 6);
    let total: usize = rows.iter().map(|r| r[1].parse::<usize>().unwrap()).sum();
    assert_eq!(total, 12);
}

#[test]
fn care_of_links_management_company() {
    let out = stdout(&run("networks", &["--link-care-of"]));
    assert_eq!(rows(&out)[0][1], "5");
}

#[test]
fn max_names_per_address_splits_generic_address() {
    let out = stdout(&run("networks", &["--max-names-per-address", "2"]));
    let sizes: Vec<String> = rows(&out).iter().map(|r| r[1].clone()).collect();
    assert_eq!(sizes, vec!["4", "2"]);
}

#[test]
fn ignore_addresses_file() {
    let dir = std::env::temp_dir().join(format!("ln-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let ignore = dir.join("ignore.txt");
    std::fs::write(&ignore, "# tax servicer\n1 Corporate Tax Way 75201\n").unwrap();
    let out = stdout(&run(
        "networks",
        &["--ignore-addresses", ignore.to_str().unwrap()],
    ));
    let sizes: Vec<String> = rows(&out).iter().map(|r| r[1].clone()).collect();
    assert_eq!(sizes, vec!["4", "2"]);
}

#[test]
fn link_names_all_merges_namesakes() {
    let out = stdout(&run("networks", &["--link-names", "all"]));
    assert!(out.contains("SMITH JOHN"));
}

#[test]
fn members_file_and_owner_occupied() {
    let dir = std::env::temp_dir().join(format!("ln-members-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let members = dir.join("members.csv");
    stdout(&run(
        "networks",
        &["--min-size", "1", "--members", members.to_str().unwrap()],
    ));
    let text = std::fs::read_to_string(&members).unwrap();
    let rows = rows(&text);
    assert_eq!(rows.len(), 12);
    let smith_home = rows.iter().find(|r| r[1] == "100-05").unwrap();
    assert_eq!(smith_home[5], "yes");
    let n1: Vec<&Vec<String>> = rows.iter().filter(|r| r[0] == "N1").collect();
    assert_eq!(n1.len(), 4);

    let out = stdout(&run(
        "networks",
        &["--min-size", "1", "--skip-owner-occupied"],
    ));
    let total: usize = super_rows_total(&out);
    assert_eq!(total, 10);
}

fn super_rows_total(out: &str) -> usize {
    rows(out)
        .iter()
        .map(|r| r[1].parse::<usize>().unwrap())
        .sum()
}

#[test]
fn lookup_by_site_address() {
    let out = stdout(&run("lookup", &["--query", "2600 W Fullerton Avenue"]));
    assert!(out.starts_with("Network N1: 4 parcels"));
    assert!(out.contains("* 100-03"));
    assert!(out.contains("100-01"));
}

#[test]
fn lookup_by_pin_and_owner() {
    let out = stdout(&run("lookup", &["--query", "100-10"]));
    assert!(out.contains("Network N2: 3 parcels"));
    let out = stdout(&run("lookup", &["--query", "fullerton partners"]));
    assert!(out.contains("Network N1"));
}

#[test]
fn lookup_no_match_fails() {
    let o = run("lookup", &["--query", "nothing like this"]);
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("no parcel matches"));
}

#[test]
fn addresses_report() {
    let out = stdout(&run("addresses", &["--top", "1"]));
    let rows = rows(&out);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0], vec!["1 CORPORATE TAX WAY 75201", "3", "3"]);
}

#[test]
fn missing_column_is_clear_error() {
    let f = fixture();
    let o = Command::new(env!("CARGO_BIN_EXE_landlord-networks"))
        .args([
            "networks",
            f.to_str().unwrap(),
            "--id",
            "NOPE",
            "--owner",
            "OWNER1",
            "--mail",
            "MAIL_ADDR",
        ])
        .output()
        .unwrap();
    assert!(!o.status.success());
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(err.contains("column 'NOPE' not found"));
    assert!(err.contains("OWNER1"));
}

#[test]
fn tab_delimited_and_latin1() {
    let dir = std::env::temp_dir().join(format!("ln-tab-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("p.tsv");
    let mut bytes = b"apn\towner\tmail\n1\tPe\xf1a Properties LLC\t5 Main St 90001\n2\tPE\xd1A PROPERTIES L.L.C.\t9 Oak St 90002\n".to_vec();
    bytes.extend_from_slice(b"3\tOther LLC\t9 Oak Street 90002\n");
    std::fs::write(&f, bytes).unwrap();
    let o = Command::new(env!("CARGO_BIN_EXE_landlord-networks"))
        .args([
            "networks",
            f.to_str().unwrap(),
            "--id",
            "apn",
            "--owner",
            "owner",
            "--mail",
            "mail",
            "--delimiter",
            "tab",
        ])
        .output()
        .unwrap();
    let out = stdout(&o);
    let rows = rows(&out);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0][1], "3");
}

#[test]
fn lookup_of_skipped_parcel_is_reported() {
    let o = run("lookup", &["--query", "100-05", "--skip-owner-occupied"]);
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("--skip-owner-occupied"));
}

#[test]
fn unmatched_ignore_address_warns() {
    let dir = std::env::temp_dir().join(format!("ln-warn-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let ignore = dir.join("ignore.txt");
    std::fs::write(&ignore, "1 Corporate Tax Way, Dallas TX 75201\n").unwrap();
    let o = run(
        "networks",
        &["--ignore-addresses", ignore.to_str().unwrap()],
    );
    assert!(o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("matches no mailing address"));
}
