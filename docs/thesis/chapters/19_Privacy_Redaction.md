# Chapter 19: Privacy and Redaction

## 19.1 Information Leakage Risks
Because `osx-clnr` logs trace events and creates plans detailing exact local paths, there is a severe risk of leaking Personally Identifiable Information (PII), such as home directory usernames, repository project names, credentials, or proprietary directory structures.

## 19.2 The Privacy Redaction Gate
We implement a Privacy Redaction Gate to sanitize raw logs before they are exported or committed to public version control. The sanitization process applies regular expression replacements:
* Matches home paths `/Users/username/` and replaces them with `/Users/<user>/`.
* Redacts credentials, tokens, and secrets from configurations.
* Obfuscates sensitive repository names in `github://` URIs.

This ensures complete compliance with repository safety standards while maintaining full structural process mining utility.
