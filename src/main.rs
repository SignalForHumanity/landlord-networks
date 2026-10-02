use clap::{Args, Parser, Subcommand, ValueEnum};
use landlord_networks::{
    Columns, LinkOptions, Linked, NameLinking, Parcel, address_stats, build_networks, find_parcels,
    normalize::normalize_address, read_parcels, suspicious_addresses, write_members,
    write_networks,
};
use std::collections::HashSet;
use std::fs::File;
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

/// Group a county parcel (assessor) CSV into landlord ownership networks:
/// parcels that share a taxpayer mailing address or an entity owner name.
#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Write a ranked list of ownership networks (CSV)
    Networks {
        #[command(flatten)]
        input: InputArgs,
        /// Write the network summary here instead of stdout
        #[arg(long, short)]
        out: Option<PathBuf>,
        /// Also write one row per parcel with its network ID
        #[arg(long)]
        members: Option<PathBuf>,
        /// Only report networks with at least this many parcels
        #[arg(long, default_value_t = 2)]
        min_size: usize,
        /// Max names/addresses listed per network in the summary (0 = all)
        #[arg(long, default_value_t = 10)]
        max_list: usize,
    },
    /// Show the network of one parcel, owner name or street address
    Lookup {
        #[command(flatten)]
        input: InputArgs,
        /// Parcel ID, or part of an owner name or site address
        #[arg(long, short)]
        query: String,
        /// Max parcels to print per network (0 = all)
        #[arg(long, default_value_t = 50)]
        limit: usize,
    },
    /// List mailing addresses shared by the most owner names, to find
    /// generic addresses (tax services, registered agents) worth ignoring
    Addresses {
        #[command(flatten)]
        input: InputArgs,
        /// Number of addresses to print
        #[arg(long, default_value_t = 25)]
        top: usize,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum NamesArg {
    /// Link on business/trust/organization names only
    Entities,
    /// Link on every owner name, including individuals
    All,
    /// Never link on names
    None,
}

#[derive(Args)]
struct InputArgs {
    /// Parcel CSV file ("-" for stdin)
    file: PathBuf,
    /// Column with the parcel ID / PIN / APN
    #[arg(long)]
    id: String,
    /// Owner-name column (repeat for OWNER1, OWNER2, ...)
    #[arg(long = "owner", required = true)]
    owners: Vec<String>,
    /// Mailing/taxpayer street-address column (repeat to concatenate lines)
    #[arg(long = "mail", required = true)]
    mail: Vec<String>,
    /// Mailing-address city column (used when no ZIP column is given)
    #[arg(long)]
    mail_city: Option<String>,
    /// Mailing-address ZIP column
    #[arg(long)]
    mail_zip: Option<String>,
    /// Property (site) street-address column, to flag owner-occupied parcels
    /// and to search with `lookup` (repeat to concatenate)
    #[arg(long = "site")]
    site: Vec<String>,
    /// Field delimiter: a single character, or "tab"
    #[arg(long, default_value = ",")]
    delimiter: String,
    /// Which owner names may link parcels on their own
    #[arg(long, value_enum, default_value_t = NamesArg::Entities)]
    link_names: NamesArg,
    /// Also link parcels through the "C/O" party (often a management company)
    #[arg(long)]
    link_care_of: bool,
    /// File of mailing addresses (one per line) never used for linking
    #[arg(long)]
    ignore_addresses: Option<PathBuf>,
    /// Do not link through addresses shared by more than N owner names
    #[arg(long)]
    max_names_per_address: Option<usize>,
    /// Ignore suite/unit numbers when comparing mailing addresses
    #[arg(long)]
    drop_unit: bool,
    /// Leave out parcels whose mailing address equals the site address
    #[arg(long)]
    skip_owner_occupied: bool,
}

/// Warn when a mailing address shared by more owner names than this is
/// still used for linking.
const GENERIC_ADDRESS_WARN: usize = 25;

fn parse_delimiter(s: &str) -> Result<u8, String> {
    match s {
        "tab" | "\\t" | "\t" => Ok(b'\t'),
        _ if s.len() == 1 && s.is_ascii() => Ok(s.as_bytes()[0]),
        _ => Err(format!(
            "delimiter must be a single ASCII character or 'tab', got '{s}'"
        )),
    }
}

fn load(input: &InputArgs) -> Result<(Vec<Parcel>, Linked), String> {
    let cols = Columns {
        id: input.id.clone(),
        owners: input.owners.clone(),
        mail: input.mail.clone(),
        mail_city: input.mail_city.clone(),
        mail_zip: input.mail_zip.clone(),
        site: input.site.clone(),
    };
    let delim = parse_delimiter(&input.delimiter)?;
    let reader: Box<dyn Read> = if input.file.as_os_str() == "-" {
        Box::new(io::stdin().lock())
    } else {
        Box::new(BufReader::new(File::open(&input.file).map_err(|e| {
            format!("cannot open {}: {e}", input.file.display())
        })?))
    };
    let parcels = read_parcels(reader, delim, &cols, input.drop_unit)?;
    let mut ignore = HashSet::new();
    if let Some(path) = &input.ignore_addresses {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(a) = normalize_address(line, None, None, input.drop_unit) {
                ignore.insert(a);
            }
        }
    }
    let opts = LinkOptions {
        names: match input.link_names {
            NamesArg::Entities => NameLinking::Entities,
            NamesArg::All => NameLinking::All,
            NamesArg::None => NameLinking::None,
        },
        care_of: input.link_care_of,
        ignore_addresses: ignore,
        max_names_per_address: input.max_names_per_address,
        skip_owner_occupied: input.skip_owner_occupied,
    };
    let known: HashSet<&str> = parcels
        .iter()
        .filter_map(|p| p.mail_key.as_deref())
        .collect();
    for a in &opts.ignore_addresses {
        if !known.contains(a.as_str()) {
            eprintln!("warning: ignore-list address '{a}' matches no mailing address in the file");
        }
    }
    let linked = build_networks(&parcels, &opts);
    let generic = suspicious_addresses(&parcels, &linked, GENERIC_ADDRESS_WARN);
    if !generic.is_empty() {
        eprintln!(
            "warning: {} mailing addresses are shared by more than {GENERIC_ADDRESS_WARN} owner names \
             and still link parcels; these are often tax/mortgage servicers or registered agents \
             that merge unrelated owners. Review them with the `addresses` command and use \
             --ignore-addresses or --max-names-per-address. Largest:",
            generic.len()
        );
        for s in generic.iter().take(5) {
            eprintln!("  {} ({} owner names)", s.address, s.owner_names);
        }
    }
    Ok((parcels, linked))
}

fn create(path: &PathBuf) -> Result<BufWriter<File>, String> {
    File::create(path)
        .map(BufWriter::new)
        .map_err(|e| format!("cannot create {}: {e}", path.display()))
}

fn run(cli: Cli) -> Result<(), String> {
    match cli.command {
        Command::Networks {
            input,
            out,
            members,
            min_size,
            max_list,
        } => {
            let (parcels, linked) = load(&input)?;
            let written = match &out {
                Some(p) => write_networks(create(p)?, &linked.networks, min_size, max_list)?,
                None => write_networks(io::stdout().lock(), &linked.networks, min_size, max_list)?,
            };
            if let Some(p) = &members {
                write_members(create(p)?, &parcels, &linked.networks, min_size)?;
            }
            let covered: usize = linked
                .networks
                .iter()
                .filter(|n| n.parcels.len() >= min_size)
                .map(|n| n.parcels.len())
                .sum();
            eprintln!(
                "{} parcels read; {written} networks with >= {min_size} parcels cover {covered} parcels",
                parcels.len()
            );
            if !linked.excluded_addresses.is_empty() {
                eprintln!(
                    "{} mailing addresses were not used for linking (ignored or over --max-names-per-address)",
                    linked.excluded_addresses.len()
                );
            }
            Ok(())
        }
        Command::Lookup {
            input,
            query,
            limit,
        } => {
            let (parcels, linked) = load(&input)?;
            let hits = find_parcels(&parcels, &query);
            if hits.is_empty() {
                return Err(format!("no parcel matches '{query}'"));
            }
            let mut nets: Vec<usize> = hits.iter().filter_map(|&i| linked.network_of[i]).collect();
            nets.sort_unstable();
            nets.dedup();
            if nets.is_empty() {
                return Err(format!(
                    "'{query}' matches only parcels left out by --skip-owner-occupied"
                ));
            }
            let stdout = io::stdout();
            let mut w = stdout.lock();
            let werr = |e: io::Error| format!("write error: {e}");
            for n in nets {
                let net = &linked.networks[n];
                writeln!(
                    w,
                    "Network {}: {} parcels, {} owner names, {} mailing addresses",
                    net.label(),
                    net.parcels.len(),
                    net.owner_names.len(),
                    net.addresses.len()
                )
                .map_err(werr)?;
                writeln!(w, "  Owner names: {}", net.owner_names.join("; ")).map_err(werr)?;
                writeln!(w, "  Mailing addresses: {}", net.addresses.join("; ")).map_err(werr)?;
                for (k, &m) in net.parcels.iter().enumerate() {
                    if limit != 0 && k >= limit {
                        writeln!(w, "  ... {} more parcels", net.parcels.len() - limit)
                            .map_err(werr)?;
                        break;
                    }
                    let p = &parcels[m];
                    let mark = if hits.contains(&m) { "*" } else { " " };
                    writeln!(
                        w,
                        " {mark} {} | {} | {} | mail: {}",
                        p.id,
                        p.site_raw,
                        p.owners_raw.join("; "),
                        p.mail_raw
                    )
                    .map_err(werr)?;
                }
                writeln!(w).map_err(werr)?;
            }
            Ok(())
        }
        Command::Addresses { input, top } => {
            let (parcels, _) = load(&input)?;
            let mut w = csv::Writer::from_writer(io::stdout().lock());
            let err = |e: csv::Error| format!("write error: {e}");
            w.write_record(["mailing_address", "owner_names", "parcels"])
                .map_err(err)?;
            for s in address_stats(&parcels).into_iter().take(top) {
                w.write_record([s.address, s.owner_names.to_string(), s.parcels.to_string()])
                    .map_err(err)?;
            }
            w.flush().map_err(|e| format!("write error: {e}"))
        }
    }
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
