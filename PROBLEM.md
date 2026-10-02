---
slug: landlord-networks
title: CLI that groups a county's parcel records into landlord ownership networks for tenant organizers
verdict: build
---

## Problem

Tenant unions grow by finding the *other* buildings their landlord owns.
Landlords often hold each building in a separate LLC, so a county assessor's
owner-name search does not show the portfolio. Volunteers rebuild the
portfolio by hand. They download the assessor's parcel file and match records
that share a taxpayer mailing address or an owner name across thousands of
rows, then check every match.

Sources:

- Chicago Reader, "How many buildings does your landlord own?"
  (https://chicagoreader.com/news/how-many-buildings-does-your-landlord-own/):
  two volunteers matched Cook County Assessor data by hand. "All of the
  properties owned by a single landlord had the same taxpayer address." The
  Saccone Renters Union "had no idea how many buildings were actually owned by
  the company."
- Shelterforce, "Tech Tools Help Tenants Push Back Against Problematic
  Landlords" (https://shelterforce.org/2025/06/17/tech-tools-help-tenants-push-back-against-problematic-landlords/):
  "A key challenge lies in scalability across geographies. Most of the tools
  we found were developed to serve tenants in a particular city or region."
- Landlord Mapper / TT4J, "national landlord database initiative"
  (https://landlordmapper.substack.com/p/welcome-to-the-national-landlord):
  "it is critical to be able to identify other properties that your landlord
  owns to grow the strength of your union."

## Who benefits

The main users are the research volunteers of tenant unions, DSA housing
committees and tenant associations in cities that have no landlord-lookup
site. Local reporters are a second group. These volunteers already download
their county's parcel CSV, which most counties publish on an open-data
portal. They would find the tool through tenant-organizing toolkits, the
Code for America / civic-tech brigade networks (for example Open Austin's
landlord-mapper effort), and GitHub. They run one prebuilt binary on a laptop
against that CSV, then open the output in a spreadsheet.

## Existing solutions

- **JustFix "Who Owns What"** (https://github.com/JustFixNYC/who-owns-what,
  https://whoownswhat.justfix.org/): excellent, but only for NYC. It is built
  on NYC HPD registration data, which other cities don't have.
- **Find My Landlord** (Chicago DSA), **Evictorbook** (Anti-Eviction Mapping
  Project, SF/Oakland/LA), **Landlord Mapper** (https://landlordmapper.org,
  Milwaukee and Chicago): each is a hosted website for one city, built by a
  dedicated team.
- **OPNDB** (https://github.com/landlordmapper/opndb): TT4J's research
  pipeline. It is Python plus Jupyter notebooks run from an IDE, its last push
  was 2025-07, and it is aimed at researchers building methods.
- **open-austin/landlord-mapper** (https://github.com/open-austin/landlord-mapper):
  a single-city project organized through Google Drive.
- **OpenRefine clustering**
  (https://openrefine.org/docs/technical-reference/clustering-in-depth): free,
  and it does merge spelling variants in *one* column. It cannot link records
  transitively: LLC A shares a mailing address with LLC B, and LLC B shares a
  name with LLC C, so all three belong together. It also cannot report the
  resulting groups as portfolios.
- **dedupe / Splink, and Rust crates `weldrs` and `zer`**: general
  probabilistic record linkage. They need programming, model configuration or
  training labels, and they have no owner-name or mailing-address
  normalization for US parcel data.
- **Commercial services** (realinfo.net, SkipTraceDepot at $10 per lookup,
  Apify scrapers): cost money, and some resell personal contact data.
- **Hand matching in a spreadsheet**: what volunteers do now. It is slow,
  error-prone, and cannot follow chains of shared addresses.

## Why build anything

Every working tool covers one city and needs a team to run a server. The one
general pipeline (OPNDB) needs Python and notebook skills. A volunteer in, say,
Columbus or Tucson who has the county parcel CSV has nothing that turns it
into "these 214 parcels under 37 LLC names share mailing addresses".

## Smallest useful intervention

A single offline binary, `landlord-networks`, that:

1. reads any parcel CSV, with the user mapping column names (parcel ID, one or
   more owner-name columns, mailing-address columns, optional site address);
2. normalizes owner names (entity suffixes such as L.L.C. becoming LLC, and
   punctuation) and mailing addresses (USPS street suffixes, PO Box forms,
   ZIP5);
3. links parcels that share a normalized mailing address or an entity owner
   name, using union-find, so chains are followed;
4. writes a ranked `networks.csv` and a per-parcel `members.csv`, and offers a
   `lookup` command ("which network is my building in?") and an `addresses`
   report that helps users spot generic addresses (tax servicers, registered
   agents) and exclude them.

No network access, no server, no personal data beyond what the user's public
file already contains.

## Success criterion

- On a synthetic fixture modeled on real assessor exports, it recovers the
  known portfolios exactly. This includes chains linked only through
  intermediate records, and namesake individuals who must stay apart. It runs
  as tests with no network access.
- It processes a 500,000-row county file in well under a minute on a laptop.
- A volunteer goes from a downloaded CSV to a ranked portfolio list with one
  command, without writing code.

## Maintenance

Small. It is pure offline CSV processing with two dependencies (`csv`,
`clap`). Things that can go wrong: counties change column names, which the
user re-maps by flag with no code change; address conventions differ outside
the US, since the suffix tables are US-centric; and generic shared addresses
can over-merge, which the tool mitigates with `--ignore-addresses`,
`--max-names-per-address` and the `addresses` report. There is no service to
keep running.

## Decision

**Build.** The need has sources (volunteers matching by hand in Chicago;
Shelterforce naming geographic scalability as the key gap). The existing tools
are tied to one city, hosted by their teams, or need Python and notebook
skills. A narrow, offline, any-county CSV-to-portfolio tool fills exactly that
gap.

On the charter's "no profiling of people": the tool only groups *property
ownership* records that counties publish, to show landlord business networks.
That is the same analysis assessor offices and journalists do. It does no
scraping, does not enrich records with contact details, and by default never
merges individual (non-entity) owners on name alone. It hides single-parcel
owners (`--min-size 2`) and can drop owner-occupied homes
(`--skip-owner-occupied`).

## Also considered

- volunteer-hours-log: free spreadsheet and Jotform/Notion templates already meet the need for small groups.
- mutual-aid-request-tracker: groups already run this successfully on Airtable and Google Sheets (Mutual Aid NYC and NY guides).
- community-garden-waitlist: city forms, PlotBase and spreadsheets cover it, and I found no sourced unmet demand.
- tefap-monthly-stats-helper: regional food banks already provide the stats spreadsheets and online forms partner pantries submit.
