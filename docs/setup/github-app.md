# 1. The GitHub App

One App per organisation or account, named after the product's service identity (the reference installation names it `spl-asdlc`). Both the CI workflows and the bridge mint tokens from it; it is the only identity that may bypass the rulesets.

**Create** (Settings → Developer settings → GitHub Apps → New):
- Name: the service identity. Homepage: the product's repository. Webhook: off (the bridge polls).
- Repository permissions: Contents **read and write** (branches, tags, merges, files), Pull requests **read and write** (reviews, comments, labels, merges), Checks **read**, Commit statuses **read**, Actions **read and write** (dispatch the cut, read runs and artifacts), Administration **read** (the required status checks of classic branch protection: without it the bridge cannot tell which checks are required and merges nothing), Metadata **read**, Workflows **read and write** only if the App must push workflow files (the bootstrap does; a running installation does not).
- Where can it be installed: only this account.
- Generate a private key; store it where the bridge runs and as the repository secret the workflows read. Note the App id and the installation id.

**Install** on each repository that adopts the method (Install App → select repositories). A repository that is not in the installation is invisible to the bridge and the workflows fail at token minting.

**Verify**: `spl-asdlc doctor --repo <owner>/<repo>` reports `github-app: ok` with the App's slug and the permissions found.

**Fallback**: where an App cannot be created (a workplace policy), the bridge accepts a fine-grained personal token with the same repository permissions. The binary prints under which login it writes; the rulesets must then list that user.
