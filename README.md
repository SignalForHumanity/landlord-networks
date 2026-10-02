# landlord-networks

Find the other buildings your landlord owns, using your county's public
parcel file.

Tenant unions grow by organizing every building a landlord owns. Landlords
often put each building in a separate LLC, so a county owner-name search
doesn't show the whole portfolio. Volunteers end up matching thousands of
assessor records by hand on a shared taxpayer mailing address
([Chicago Reader](https://chicagoreader.com/news/how-many-buildings-does-your-landlord-own/)).
Tools that automate this, such as Who Owns What, Find My Landlord, Evictorbook
and Landlord Mapper, each cover a single city.

`landlord-networks` is one offline program that works on **any** county's
parcel CSV. It links parcels that share a normalized **mailing address** or a
normalized **entity owner name**, follows chains of links (LLC A shares an
address with LLC B, and LLC B shares a name with LLC C), and writes a ranked
list of ownership networks you can open in a spreadsheet.

The reasoning behind the tool, including the existing alternatives, is in
[PROBLEM.md](PROBLEM.md).

## Who it's for

- Research volunteers in tenant unions and tenant associations
- Housing committees and local reporters in places with no landlord-lookup
  site

You need to be able to download a CSV and run one command in a terminal. You
don't need to write any code.

## Install

With a Rust toolchain:

```sh
cargo install --git https://github.com/SignalForHumanity/landlord-networks
# or, from a checkout:
cargo build --release   # binary at target/release/landlord-networks
```

It is a single self-contained binary with no runtime dependencies and needs
no network access.

## Get the data

Download your county assessor's or treasurer's parcel / tax roll file. Many
counties publish it on their open-data portal. Look for "parcels", "tax roll",
"assessor", "property characteristics" or "owner". You need at least:

- a parcel ID column (PIN, APN, PARCEL_ID)
- one or more owner-name columns
- the **mailing / taxpayer address** columns (not the property's own address)

A site (property) address column is optional but recommended.

## Usage

Map your file's column names with flags. Matching ignores case.

```sh
landlord-networks networks parcels.csv \
  --id PIN --owner OWNER_NAME --owner OWNER_NAME2 \
  --mail MAIL_ADDR1 --mail MAIL_ADDR2 --mail-city MAIL_CITY --mail-zip MAIL_ZIP \
  --site SITE_ADDRESS \
  --out networks.csv --members members.csv
```

`networks.csv` has one row per network, largest first:

| network_id | parcels | owner_name_count | address_count | owner_occupied_parcels | owner_names | mailing_addresses |
|---|---|---|---|---|---|---|
| N1 | 4 | 3 | 2 | 0 | LOGAN SQ HOLDINGS LLC; 2400 MILWAUKEE LLC; FULLERTON PARTNERS LP | 350 W HUBBARD ST # 300 60654; PO BOX 4410 60680 |

`members.csv` has one row per parcel with its `network_id`. Filter it in a
spreadsheet to get a building list for outreach.

### Which network is my building in?

```sh
landlord-networks lookup parcels.csv --id PIN --owner OWNER_NAME \
  --mail MAIL_ADDR1 --mail-zip MAIL_ZIP --site SITE_ADDRESS \
  --query "2600 W Fullerton Ave"
```

```
Network N1: 4 parcels, 3 owner names, 2 mailing addresses
  Owner names: LOGAN SQ HOLDINGS LLC; 2400 MILWAUKEE LLC; FULLERTON PARTNERS LP
  Mailing addresses: 350 W HUBBARD ST # 300 60654; PO BOX 4410 60680
   100-01 | 2400 N Milwaukee Ave | 2400 Milwaukee LLC | mail: 350 W. Hubbard Street, Suite 300 Chicago 60654-1234
 * 100-03 | 2600 W Fullerton Ave | LOGAN SQ HOLDINGS LLC | mail: PO Box 4410 Chicago 60680
   ...
```

`--query` accepts a parcel ID, part of an owner name, or whole words of a
site address ("12 Elm St" does not match "112 Elm St"). Variants like
"Avenue" vs "Ave" are handled.

### Check for over-merging first

Some mailing addresses are shared by owners who are **not** related: tax
payment services, registered agents, banks, government offices. List the
addresses with the most distinct owner names:

```sh
landlord-networks addresses parcels.csv --id PIN --owner OWNER_NAME \
  --mail MAIL_ADDR1 --mail-zip MAIL_ZIP --top 40
```

Look up any surprising ones. Then put the ones to exclude in a text file, one
per line (lines starting with `#` are comments; you can paste the normalized
form straight from the report), and pass `--ignore-addresses ignore.txt`. You
can also use `--max-names-per-address N` to skip any address shared by more
than N owner names automatically.

`networks` and `lookup` print a warning when an address shared by more than
25 owner names is still used for linking, and when a line in the ignore file
matches no address in the data. Mortgage escrow and tax servicers often
receive the tax bills of many private homeowners; left in, they make those
homeowners look like one landlord.

### Options

| Flag | Effect |
|---|---|
| `--link-names entities` (default) | Link on business, trust and organization names only. Two private individuals called "SMITH JOHN" are **not** merged unless they share an address. |
| `--link-names all` / `none` | Link on every name, or only on addresses. |
| `--link-care-of` | Also link through the "C/O" party, often a management company. This gives *managed-by* networks, not strict ownership. |
| `--drop-unit` | Ignore suite numbers, so "STE 200" and "STE 300" at one address match. Do not use it on condo buildings: it merges every unit owner who lives there. |
| `--skip-owner-occupied` | Leave out parcels whose mailing address equals the site address (homeowners). |
| `--min-size N` | Only report networks with at least N parcels (default 2). |
| `--delimiter tab` | For tab- or pipe-separated files (`--delimiter '|'`). |

Input can be `-` for stdin. Cells that are not valid UTF-8 are read as
Latin-1.
Rows with the same parcel ID are merged.

## How matching works

- **Owner names**: uppercased, punctuation removed, and entity forms made
  canonical: `L.L.C.` and `Limited Liability Company` become `LLC`,
  `Incorporated` becomes `INC`, `Corporation` becomes `CORP`, `Trustee`
  becomes `TR`, `ET AL` is dropped, and a leading `THE` is dropped. Anything
  after `C/O`, `ATTN` or `%` is split off as the care-of party; a `C/O` line
  in its own owner column is attached to the owner before it.
- **Entity or individual**: a name counts as an entity when it contains a
  word such as LLC, INC, TRUST, PROPERTIES or HOLDINGS. A person named as
  trustee ("SMITH JOHN TR") or with the surname Church is treated as an
  individual. With `--link-care-of`, a care-of party that is a private
  individual is only linked with `--link-names all`.
- **Mailing addresses**: USPS abbreviations (Street → ST, North → N,
  Suite/Unit/Apt → `#`), the PO Box forms (`P.O. Box`, `Post Office Box`,
  `POB`) unified, ZIP+4 cut to ZIP5, with ZIP (or city, if there is no ZIP
  column) appended.
- Parcels sharing either key are grouped with union-find, so links are
  transitive.

## Limitations

- **Treat results as leads, not proof.** A shared address can be a
  coincidence, and the tool can't see owners that use different addresses
  *and* different names. Check results against the Secretary of State
  business registry, the recorder of deeds and rental registrations before
  publishing.
- Linking through state business filings (registered agents, officers) is not
  done. Those files vary by state and are often not available in bulk.
- The address rules are US-centric. Addresses written in very different forms
  (for example "Hubbard St" vs "W Hubbard St") will not match.
- Owner-occupied detection needs the mailing street to be in its own column
  (not combined with city and ZIP). City and ZIP are not compared, so the
  same street address in another town also counts as owner-occupied.
- Government and housing-authority owners also form large networks. That is
  expected; filter them out in the spreadsheet if you don't need them.

## Responsible use

This tool only reorganizes property records the county already publishes. It
does not scrape, and it adds no phone numbers, emails or other personal data.
By default it never merges private individuals on name alone and it hides
single-parcel owners. Use it to understand landlord businesses, not to harass
anyone.

## Development

```sh
cargo test                       # unit + CLI tests, no network
cargo clippy --all-targets -- -D warnings
```

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE),
at your option.
