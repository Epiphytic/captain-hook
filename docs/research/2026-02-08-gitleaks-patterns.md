# Gitleaks Regex Patterns for Rust RegexSet

**Date:** 2026-02-08
**Source:** [gitleaks/gitleaks](https://github.com/gitleaks/gitleaks) `config/gitleaks.toml`
**Purpose:** Curated patterns for hookwise's Tier 1 RegexSet secret detection

## Overview

~55 patterns selected from gitleaks' 200+ rules, prioritized by:
1. Prevalence in real codebases
2. Severity of exposure
3. Low false-positive rate
4. Suitability for Rust `regex` crate RegexSet (no lookaround)

Patterns are grouped into three tiers matching hookwise's sanitization architecture:
- **Aho-Corasick literals** (Tier 0) -- prefix-matchable tokens handled before RegexSet
- **RegexSet patterns** (Tier 1) -- the patterns in this document
- **Shannon entropy** (Tier 2) -- fallback for unknown formats

## Rust Compatibility Notes

The Rust `regex` crate does NOT support:
- Lookahead/lookbehind (`(?=...)`, `(?<=...)`, `(?!...)`, `(?<!...)`)
- Backreferences (`\1`)
- Atomic groups

Gitleaks patterns use `(?i)` (case-insensitive) and `(?-i:)` (case-sensitive inline) which ARE supported by the Rust `regex` crate. The `\b` word boundary and `[\x60]` hex escapes are also supported.

Many gitleaks patterns use a common suffix: `(?:[\x60'"\s;]|\\[nr]|$)` to anchor the end of a captured secret. This is Rust-compatible.

---

## Category 1: Cloud Providers (AWS, GCP, Azure)

### P01: AWS Access Key ID

| Field | Value |
|-------|-------|
| Pattern | `\b((?:A3T[A-Z0-9]\|AKIA\|ASIA\|ABIA\|ACCA)[A-Z2-7]{16})\b` |
| Detects | AWS access key IDs (permanent AKIA, temporary ASIA, STS ABIA, etc.) |
| Category | AWS / IAM |
| False positive risk | **Low** -- 4-char prefix + base32 is highly specific |

### P02: GCP API Key

| Field | Value |
|-------|-------|
| Pattern | `\b(AIza[\w-]{35})(?:[\x60'"\s;]\|\\[nr]\|$)` |
| Detects | Google Cloud Platform API keys |
| Category | GCP |
| False positive risk | **Low** -- `AIza` prefix is unique to Google |

### P03: Azure AD Client Secret

| Field | Value |
|-------|-------|
| Pattern | `(?:^\|[\\'"\x60\s>=:(,)])([a-zA-Z0-9_~.]{3}\dQ~[a-zA-Z0-9_~.-]{31,34})(?:$\|[\\'"\x60\s<),])` |
| Detects | Azure Active Directory client secrets |
| Category | Azure / Entra ID |
| False positive risk | **Medium** -- requires `Q~` in specific position, but context-dependent |

### P04: AWS Bedrock Long-Lived API Key

| Field | Value |
|-------|-------|
| Pattern | `\b(ABSK[A-Za-z0-9+/]{109,269}={0,2})\b` |
| Detects | Amazon Bedrock API keys (long-lived) |
| Category | AWS / Bedrock |
| False positive risk | **Low** -- `ABSK` prefix + long base64 body |

---

## Category 2: Code Hosting (GitHub, GitLab, Bitbucket)

### P05: GitHub Personal Access Token

| Field | Value |
|-------|-------|
| Pattern | `ghp_[0-9a-zA-Z]{36}` |
| Detects | GitHub classic personal access tokens |
| Category | GitHub |
| False positive risk | **Low** -- `ghp_` prefix is unique |

### P06: GitHub OAuth Token

| Field | Value |
|-------|-------|
| Pattern | `gho_[0-9a-zA-Z]{36}` |
| Detects | GitHub OAuth access tokens |
| Category | GitHub |
| False positive risk | **Low** |

### P07: GitHub App Token (ghu/ghs)

| Field | Value |
|-------|-------|
| Pattern | `(?:ghu\|ghs)_[0-9a-zA-Z]{36}` |
| Detects | GitHub user-to-server and server-to-server tokens |
| Category | GitHub |
| False positive risk | **Low** |

### P08: GitHub Fine-Grained PAT

| Field | Value |
|-------|-------|
| Pattern | `github_pat_\w{82}` |
| Detects | GitHub fine-grained personal access tokens |
| Category | GitHub |
| False positive risk | **Low** -- long prefix + fixed length |

### P09: GitLab Personal Access Token

| Field | Value |
|-------|-------|
| Pattern | `glpat-[\w-]{20}` |
| Detects | GitLab personal access tokens |
| Category | GitLab |
| False positive risk | **Low** |

### P10: GitLab Service Account Token

| Field | Value |
|-------|-------|
| Pattern | `(?i)\b(glsa_[A-Za-z0-9]{32}_[A-Fa-f0-9]{8})(?:[\x60'"\s;]\|\\[nr]\|$)` |
| Detects | GitLab service account tokens |
| Category | GitLab |
| False positive risk | **Low** |

### P11: Bitbucket Client Secret

| Field | Value |
|-------|-------|
| Pattern | `(?i)[\w.-]{0,50}?(?:bitbucket)(?:[ \t\w.-]{0,20})[\s'"]{0,3}(?:=\|>\|:{1,3}=\|\\\|\\\|\|:\|=>\|\?=\|,)[\x60'"\s=]{0,5}([a-z0-9=_\-]{64})(?:[\x60'"\s;]\|\\[nr]\|$)` |
| Detects | Bitbucket client secrets |
| Category | Bitbucket |
| False positive risk | **Medium** -- keyword-anchored, 64-char hex |

---

## Category 3: Messaging & Collaboration (Slack, Discord, Telegram)

### P12: Slack Bot Token

| Field | Value |
|-------|-------|
| Pattern | `xoxb-[0-9]{10,13}-[0-9]{10,13}[a-zA-Z0-9-]*` |
| Detects | Slack bot tokens |
| Category | Slack |
| False positive risk | **Low** -- `xoxb-` prefix + numeric segments |

### P13: Slack User Token

| Field | Value |
|-------|-------|
| Pattern | `xox[pe](?:-[0-9]{10,13}){3}-[a-zA-Z0-9-]{28,34}` |
| Detects | Slack user tokens (xoxp) and enterprise tokens (xoxe) |
| Category | Slack |
| False positive risk | **Low** |

### P14: Slack Webhook URL

| Field | Value |
|-------|-------|
| Pattern | `(?:https?://)?hooks\.slack\.com/(?:services\|workflows\|triggers)/[A-Za-z0-9+/]{43,56}` |
| Detects | Slack incoming webhook URLs |
| Category | Slack |
| False positive risk | **Low** -- domain-anchored |

### P15: Discord API Token

| Field | Value |
|-------|-------|
| Pattern | `(?i)[\w.-]{0,50}?(?:discord)(?:[ \t\w.-]{0,20})[\s'"]{0,3}(?:=\|>\|:{1,3}=\|\\\|\\\|\|:\|=>\|\?=\|,)[\x60'"\s=]{0,5}([a-f0-9]{64})(?:[\x60'"\s;]\|\\[nr]\|$)` |
| Detects | Discord API tokens |
| Category | Discord |
| False positive risk | **Medium** -- keyword-anchored, 64-char hex could match other things |

### P16: Telegram Bot Token

| Field | Value |
|-------|-------|
| Pattern | `(?i)[\w.-]{0,50}?(?:telegr)(?:[ \t\w.-]{0,20})[\s'"]{0,3}(?:=\|>\|:{1,3}=\|\\\|\\\|\|:\|=>\|\?=\|,)[\x60'"\s=]{0,5}([0-9]{5,16}:(?-i:A)[a-z0-9_\-]{34})(?:[\x60'"\s;]\|\\[nr]\|$)` |
| Detects | Telegram Bot API tokens |
| Category | Telegram |
| False positive risk | **Low** -- numeric ID + `:A` + base62 is distinctive |

---

## Category 4: AI/ML Providers

### P17: Anthropic API Key

| Field | Value |
|-------|-------|
| Pattern | `\b(sk-ant-api03-[a-zA-Z0-9_\-]{93}AA)(?:[\x60'"\s;]\|\\[nr]\|$)` |
| Detects | Anthropic Claude API keys |
| Category | Anthropic |
| False positive risk | **Low** -- long unique prefix |

### P18: Anthropic Admin API Key

| Field | Value |
|-------|-------|
| Pattern | `\b(sk-ant-admin01-[a-zA-Z0-9_\-]{93}AA)(?:[\x60'"\s;]\|\\[nr]\|$)` |
| Detects | Anthropic admin API keys |
| Category | Anthropic |
| False positive risk | **Low** |

### P19: OpenAI API Key

| Field | Value |
|-------|-------|
| Pattern | `\b(sk-(?:proj\|svcacct\|admin)-(?:[A-Za-z0-9_-]{74}\|[A-Za-z0-9_-]{58})T3BlbkFJ(?:[A-Za-z0-9_-]{74}\|[A-Za-z0-9_-]{58})\b\|sk-[a-zA-Z0-9]{20}T3BlbkFJ[a-zA-Z0-9]{20})(?:[\x60'"\s;]\|\\[nr]\|$)` |
| Detects | OpenAI API keys (project, service account, admin, and legacy) |
| Category | OpenAI |
| False positive risk | **Low** -- `T3BlbkFJ` (base64 of "OpenAI") is highly specific |

### P20: Hugging Face Access Token

| Field | Value |
|-------|-------|
| Pattern | `\b(hf_(?i:[a-z]{34}))(?:[\x60'"\s;]\|\\[nr]\|$)` |
| Detects | Hugging Face access tokens |
| Category | Hugging Face |
| False positive risk | **Low** -- `hf_` prefix |

### P21: Hugging Face Organization Token

| Field | Value |
|-------|-------|
| Pattern | `\b(api_org_(?i:[a-z]{34}))(?:[\x60'"\s;]\|\\[nr]\|$)` |
| Detects | Hugging Face organization API tokens |
| Category | Hugging Face |
| False positive risk | **Low** |

---

## Category 5: Payment Processors (Stripe, Square, Flutterwave)

### P22: Stripe Secret/Restricted Key

| Field | Value |
|-------|-------|
| Pattern | `\b((?:sk\|rk)_(?:test\|live\|prod)_[a-zA-Z0-9]{10,99})(?:[\x60'"\s;]\|\\[nr]\|$)` |
| Detects | Stripe secret keys (sk_live_, sk_test_) and restricted keys (rk_live_) |
| Category | Stripe |
| False positive risk | **Low** -- distinctive prefix pattern |

### P23: Square Access Token

| Field | Value |
|-------|-------|
| Pattern | `\b((?:EAAA\|sq0atp-)[\w-]{22,60})\b` |
| Detects | Square OAuth and API access tokens |
| Category | Square |
| False positive risk | **Low** |

### P24: Flutterwave Secret Key

| Field | Value |
|-------|-------|
| Pattern | `FLWSECK_TEST-(?i)[a-h0-9]{32}-X` |
| Detects | Flutterwave test secret keys |
| Category | Flutterwave |
| False positive risk | **Low** |

---

## Category 6: DevOps & Infrastructure

### P25: HashiCorp Vault Service Token

| Field | Value |
|-------|-------|
| Pattern | `\b((?:hvs\.[\w-]{90,120}\|s\.(?i:[a-z0-9]{24})))(?:[\x60'"\s;]\|\\[nr]\|$)` |
| Detects | Vault service tokens (new hvs. and legacy s. format) |
| Category | HashiCorp Vault |
| False positive risk | **Low** for hvs., **Medium** for legacy s. format |

### P26: HashiCorp Vault Batch Token

| Field | Value |
|-------|-------|
| Pattern | `\b(hvb\.[\w-]{138,300})(?:[\x60'"\s;]\|\\[nr]\|$)` |
| Detects | Vault batch tokens |
| Category | HashiCorp Vault |
| False positive risk | **Low** |

### P27: Pulumi API Token

| Field | Value |
|-------|-------|
| Pattern | `\b(pul-[a-f0-9]{40})\b` |
| Detects | Pulumi access tokens |
| Category | Pulumi |
| False positive risk | **Low** |

### P28: Databricks API Token

| Field | Value |
|-------|-------|
| Pattern | `\b(dapi[a-f0-9]{32}(?:-\d)?)\b` |
| Detects | Databricks personal access tokens |
| Category | Databricks |
| False positive risk | **Low** -- `dapi` prefix + hex body |

### P29: Grafana Cloud API Token

| Field | Value |
|-------|-------|
| Pattern | `(?i)\b(glc_[A-Za-z0-9+/]{32,400}={0,3})\b` |
| Detects | Grafana Cloud API tokens |
| Category | Grafana |
| False positive risk | **Low** |

### P30: Grafana Service Account Token

| Field | Value |
|-------|-------|
| Pattern | `(?i)\b(glsa_[A-Za-z0-9]{32}_[A-Fa-f0-9]{8})\b` |
| Detects | Grafana service account tokens |
| Category | Grafana |
| False positive risk | **Low** |

### P31: Heroku API Key v2

| Field | Value |
|-------|-------|
| Pattern | `\b((HRKU-AA[0-9a-zA-Z_-]{58}))(?:[\x60'"\s;]\|\\[nr]\|$)` |
| Detects | Heroku API keys (new format) |
| Category | Heroku |
| False positive risk | **Low** |

---

## Category 7: Cloud Platforms & Hosting

### P32: DigitalOcean Personal Access Token

| Field | Value |
|-------|-------|
| Pattern | `\b(dop_v1_[a-f0-9]{64})(?:[\x60'"\s;]\|\\[nr]\|$)` |
| Detects | DigitalOcean personal access tokens |
| Category | DigitalOcean |
| False positive risk | **Low** |

### P33: DigitalOcean OAuth Access Token

| Field | Value |
|-------|-------|
| Pattern | `\b(doo_v1_[a-f0-9]{64})(?:[\x60'"\s;]\|\\[nr]\|$)` |
| Detects | DigitalOcean OAuth tokens |
| Category | DigitalOcean |
| False positive risk | **Low** |

### P34: Cloudflare Origin CA Key

| Field | Value |
|-------|-------|
| Pattern | `\b(v1\.0-[a-f0-9]{24}-[a-f0-9]{146})(?:[\x60'"\s;]\|\\[nr]\|$)` |
| Detects | Cloudflare Origin CA keys |
| Category | Cloudflare |
| False positive risk | **Low** -- distinctive structure |

### P35: Shopify Access Token

| Field | Value |
|-------|-------|
| Pattern | `shpat_[a-fA-F0-9]{32}` |
| Detects | Shopify admin API access tokens |
| Category | Shopify |
| False positive risk | **Low** |

### P36: Shopify Custom App Token

| Field | Value |
|-------|-------|
| Pattern | `shpca_[a-fA-F0-9]{32}` |
| Detects | Shopify custom app access tokens |
| Category | Shopify |
| False positive risk | **Low** |

### P37: Shopify Shared Secret

| Field | Value |
|-------|-------|
| Pattern | `shpss_[a-fA-F0-9]{32}` |
| Detects | Shopify shared secrets |
| Category | Shopify |
| False positive risk | **Low** |

---

## Category 8: Package Registries

### P38: npm Access Token

| Field | Value |
|-------|-------|
| Pattern | `(?i)\b(npm_[a-z0-9]{36})(?:[\x60'"\s;]\|\\[nr]\|$)` |
| Detects | npm access tokens |
| Category | npm |
| False positive risk | **Low** |

### P39: PyPI Upload Token

| Field | Value |
|-------|-------|
| Pattern | `pypi-AgEIcHlwaS5vcmc[\w-]{50,1000}` |
| Detects | PyPI API tokens |
| Category | PyPI |
| False positive risk | **Low** -- base64 prefix encodes "pypi.org" |

---

## Category 9: Email & Marketing (SendGrid, Mailchimp, Twilio)

### P40: SendGrid API Token

| Field | Value |
|-------|-------|
| Pattern | `\b(SG\.(?i)[a-z0-9=_\-\.]{66})(?:[\x60'"\s;]\|\\[nr]\|$)` |
| Detects | SendGrid API keys |
| Category | SendGrid |
| False positive risk | **Low** -- `SG.` prefix |

### P41: Twilio API Key

| Field | Value |
|-------|-------|
| Pattern | `SK[0-9a-fA-F]{32}` |
| Detects | Twilio API keys |
| Category | Twilio |
| False positive risk | **Medium** -- `SK` + 32 hex is somewhat generic; may match non-Twilio strings. Add keyword anchor in practice. |

### P42: Mailchimp API Key

| Field | Value |
|-------|-------|
| Pattern | `(?i)[\w.-]{0,50}?(?:MailchimpSDK\.initialize\|mailchimp)(?:[ \t\w.-]{0,20})[\s'"]{0,3}(?:=\|>\|:{1,3}=\|\\\|\\\|\|:\|=>\|\?=\|,)[\x60'"\s=]{0,5}([a-f0-9]{32}-us\d\d)(?:[\x60'"\s;]\|\\[nr]\|$)` |
| Detects | Mailchimp API keys (hex + datacenter suffix) |
| Category | Mailchimp |
| False positive risk | **Low** -- keyword-anchored + `-us` datacenter suffix |

### P43: Datadog Access Token

| Field | Value |
|-------|-------|
| Pattern | `(?i)[\w.-]{0,50}?(?:datadog)(?:[ \t\w.-]{0,20})[\s'"]{0,3}(?:=\|>\|:{1,3}=\|\\\|\\\|\|:\|=>\|\?=\|,)[\x60'"\s=]{0,5}([a-z0-9]{40})(?:[\x60'"\s;]\|\\[nr]\|$)` |
| Detects | Datadog API and application keys |
| Category | Datadog |
| False positive risk | **Medium** -- 40-char lowercase hex is common; keyword anchor helps |

---

## Category 10: SaaS & Productivity

### P44: Atlassian API Token (ATATT format)

| Field | Value |
|-------|-------|
| Pattern | `ATATT3[A-Za-z0-9_\-=]{186}` |
| Detects | Atlassian API tokens (new ATATT format) |
| Category | Atlassian / Jira / Confluence |
| False positive risk | **Low** -- unique prefix + long fixed length |

### P45: Notion API Token

| Field | Value |
|-------|-------|
| Pattern | `\b(ntn_[0-9]{11}[A-Za-z0-9]{32}[A-Za-z0-9]{3})\b` |
| Detects | Notion integration tokens |
| Category | Notion |
| False positive risk | **Low** |

### P46: Linear API Key

| Field | Value |
|-------|-------|
| Pattern | `lin_api_(?i)[a-z0-9]{40}` |
| Detects | Linear API keys |
| Category | Linear |
| False positive risk | **Low** |

### P47: Postman API Token

| Field | Value |
|-------|-------|
| Pattern | `\b(PMAK-(?i)[a-f0-9]{24}\-[a-f0-9]{34})\b` |
| Detects | Postman API keys |
| Category | Postman |
| False positive risk | **Low** |

### P48: Airtable Personal Access Token

| Field | Value |
|-------|-------|
| Pattern | `\b(pat[[:alnum:]]{14}\.[a-f0-9]{64})\b` |
| Detects | Airtable personal access tokens |
| Category | Airtable |
| False positive risk | **Low** -- `pat` + dot-separated structure |

### P49: PlanetScale API Token

| Field | Value |
|-------|-------|
| Pattern | `\b(pscale_tkn_(?i)[\w=\.-]{32,64})(?:[\x60'"\s;]\|\\[nr]\|$)` |
| Detects | PlanetScale API tokens |
| Category | PlanetScale |
| False positive risk | **Low** |

### P50: Doppler API Token

| Field | Value |
|-------|-------|
| Pattern | `dp\.pt\.(?i)[a-z0-9]{43}` |
| Detects | Doppler service tokens |
| Category | Doppler |
| False positive risk | **Low** |

---

## Category 11: Tokens & Cryptographic Material

### P51: JSON Web Token (JWT)

| Field | Value |
|-------|-------|
| Pattern | `\b(ey[a-zA-Z0-9]{17,}\.ey[a-zA-Z0-9\/\\_-]{17,}\.(?:[a-zA-Z0-9\/\\_-]{10,}={0,2})?)(?:[\x60'"\s;]\|\\[nr]\|$)` |
| Detects | JSON Web Tokens (header.payload.signature) |
| Category | JWT / Generic Auth |
| False positive risk | **Medium** -- `ey` prefix (base64 of `{`) is common in non-secret JWTs too. May want to filter for known non-secrets. |

### P52: Private Key (PEM format)

| Field | Value |
|-------|-------|
| Pattern | `(?i)-----BEGIN[ A-Z0-9_-]{0,100}PRIVATE KEY(?: BLOCK)?-----[\s\S-]{64,}?KEY(?: BLOCK)?-----` |
| Detects | SSH, RSA, DSA, EC, PGP private keys in PEM format |
| Category | Cryptographic Keys |
| False positive risk | **Low** -- PEM header/footer is unambiguous. Note: requires multiline mode in Rust regex or `(?s)` flag. |

### P53: Age Secret Key

| Field | Value |
|-------|-------|
| Pattern | `AGE-SECRET-KEY-1[QPZRY9X8GF2TVDW0S3JN54KHCE6MUA7L]{58}` |
| Detects | age encryption secret keys (bech32 encoding) |
| Category | Encryption |
| False positive risk | **Low** -- bech32 charset + exact prefix |

---

## Category 12: Generic / Contextual Patterns

### P54: Generic API Key (keyword-anchored)

| Field | Value |
|-------|-------|
| Pattern | `(?i)[\w.-]{0,50}?(?:access\|auth\|(?-i:[Aa]pi\|API)\|credential\|creds\|key\|passw(?:or)?d\|secret\|token)(?:[ \t\w.-]{0,20})[\s'"]{0,3}(?:=\|>\|:{1,3}=\|\\\|\\\|\|:\|=>\|\?=\|,)[\x60'"\s=]{0,5}([\w.=-]{10,150}\|[a-z0-9][a-z0-9+/]{11,}={0,3})(?:[\x60'"\s;]\|\\[nr]\|$)` |
| Detects | Generic secrets assigned to keyword-named variables (api_key=, password=, secret=, etc.) |
| Category | Generic |
| False positive risk | **High** -- extremely broad. Use as last resort. Requires aggressive allowlisting for known non-secrets (e.g., `password=********`, placeholder values). |

### P55: Codecov Access Token

| Field | Value |
|-------|-------|
| Pattern | `(?i)[\w.-]{0,50}?(?:codecov)(?:[ \t\w.-]{0,20})[\s'"]{0,3}(?:=\|>\|:{1,3}=\|\\\|\\\|\|:\|=>\|\?=\|,)[\x60'"\s=]{0,5}([a-z0-9]{32})(?:[\x60'"\s;]\|\\[nr]\|$)` |
| Detects | Codecov upload tokens |
| Category | CI/CD |
| False positive risk | **Medium** -- keyword-anchored, 32-char hex |

---

## Implementation Recommendations

### Aho-Corasick Literals (Tier 0, handled before RegexSet)

These prefixes should be matched via aho-corasick for ~5us detection before the RegexSet even runs:

```
sk-ant-api03-     # Anthropic
sk-ant-admin01-   # Anthropic admin
ghp_              # GitHub PAT
gho_              # GitHub OAuth
ghs_              # GitHub server-to-server
ghu_              # GitHub user-to-server
github_pat_       # GitHub fine-grained
glpat-            # GitLab PAT
glsa_             # GitLab service account
xoxb-             # Slack bot
xoxp-             # Slack user
xoxe-             # Slack enterprise
AKIA              # AWS permanent
ASIA              # AWS temporary
ABIA              # AWS STS
ACCA              # AWS
ABSK              # AWS Bedrock
AIza              # GCP
shpat_            # Shopify
shpca_            # Shopify custom
shpss_            # Shopify shared secret
npm_              # npm
pypi-AgEI         # PyPI
SG.               # SendGrid
FLWSECK_TEST-     # Flutterwave
FLWPUBK_TEST-     # Flutterwave
hvs.              # Vault service
hvb.              # Vault batch
dapi              # Databricks
glc_              # Grafana cloud
HRKU-AA           # Heroku
dop_v1_           # DigitalOcean PAT
doo_v1_           # DigitalOcean OAuth
dor_v1_           # DigitalOcean refresh
pul-              # Pulumi
PMAK-             # Postman
lin_api_          # Linear
dp.pt.            # Doppler
pscale_tkn_       # PlanetScale
pscale_oauth_     # PlanetScale
ntn_              # Notion
ATATT3            # Atlassian
AGE-SECRET-KEY-1  # age encryption
-----BEGIN        # PEM private key header
sq0atp-           # Square
```

### RegexSet Compilation Order

For the Rust `RegexSet`, compile patterns in this priority order:
1. **Provider-specific with unique prefixes** (P01-P50) -- low false positive, fast reject
2. **Structural patterns** (P51 JWT, P52 PEM, P53 age) -- distinctive format
3. **Keyword-anchored generics** (P54, P55) -- high false positive, last resort

### Shannon Entropy Fallback (Tier 2)

For strings 20+ characters that pass through Tiers 0 and 1 undetected:
- Calculate Shannon entropy
- Flag if entropy > 4.0 bits/char AND string length >= 20
- This catches custom/unknown token formats

### Allowlist Recommendations

Common false positives to allowlist:
- `AKIAIOSFODNN7EXAMPLE` (AWS example key from docs)
- `wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY` (AWS example secret)
- Placeholder values: `********`, `xxxx`, `<your-key-here>`, `TODO`, `CHANGEME`
- Test fixtures and mock data paths
- Base64-encoded non-secret content (e.g., `eyJ0eXAiOiJKV1QiLCJhbGciOiJub25lIn0` is an unsigned JWT header)
