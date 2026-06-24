#!/usr/bin/env bash
# check-signer.sh — verify that the Tauri updater signing (key, password) pair
# you have in hand actually decrypts and signs. Exits 0 on success, non-zero
# on a specific failure so scripts (and you) can react.
#
# Why this exists: the Tauri build step only reports the wrong-password error
# *after* a 4-5 minute native compile. This script catches it in ~2 seconds
# before you waste a CI runner.
#
# Usage:
#   # 1. From env (the way CI uses them):
#   export TAURI_SIGNING_PRIVATE_KEY=...
#   export TAURI_SIGNING_PRIVATE_KEY_PASSWORD=...
#   ./scripts/check-signer.sh --from-env
#
#   # 2. From local files (the most common dev case):
#   ./scripts/check-signer.sh --key /path/to/nicessh.key --password /path/to/nicessh.pwd
#
#   # 3. From stdin (for piping, e.g. `gh secret get` style tools):
#   cat nicessh.key     | ./scripts/check-signer.sh --key-stdin
#   cat nicessh.pwd.txt | ./scripts/check-signer.sh --password-stdin
#
# Exit codes:
#   0  pair is valid (signer wrote a .sig)
#   2  password is wrong for that key
#   3  key file is not a valid Tauri signing key (base64 / CRLF / etc.)
#   4  required input missing (env unset, or --key/--password path)
#   1  other failure (network, tauri cli, timeout, …)
#
# Safety: never prints the key or password contents; cleans up the temp
# signing payload on exit.

set -u
set -o pipefail

# ---------- args ----------
MODE_ENV=0
MODE_KEY_FILE=""
MODE_KEY_STDIN=0
MODE_PWD_FILE=""
MODE_PWD_STDIN=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --from-env)         MODE_ENV=1; shift ;;
    --key)              [[ $# -ge 2 ]] || { echo "--key needs a value" >&2; exit 4; }; MODE_KEY_FILE="$2"; shift 2 ;;
    --key-stdin)        MODE_KEY_STDIN=1; shift ;;
    --password)         [[ $# -ge 2 ]] || { echo "--password needs a value" >&2; exit 4; }; MODE_PWD_FILE="$2"; shift 2 ;;
    --password-stdin)   MODE_PWD_STDIN=1; shift ;;
    -h|--help)          sed -n '2,28p' "$0"; exit 0 ;;
    *)                  echo "unknown arg: $1" >&2; exit 4 ;;
  esac
done

# ---------- colors (no-op when not a tty) ----------
if [[ -t 1 ]]; then
  RED=$'\033[31m'; GRN=$'\033[32m'; YEL=$'\033[33m'; DIM=$'\033[2m'; RST=$'\033[0m'
else
  RED=""; GRN=""; YEL=""; DIM=""; RST=""
fi

# ---------- 1. resolve key + password into temp files ----------
WORK=$(mktemp -d -t nicessh-signer-check.XXXXXX)
trap 'rm -rf "$WORK"' EXIT
chmod 700 "$WORK"

KEY_PATH="$WORK/key"
PWD_PATH="$WORK/password"

resolve_from_env() {
  # Only fail if the variable is literally unset; an empty password is a valid
  # signing password (the Tauri signer accepts -p "" for unencrypted keys).
  if [[ -z "${TAURI_SIGNING_PRIVATE_KEY+x}" || -z "${TAURI_SIGNING_PRIVATE_KEY_PASSWORD+x}" ]]; then
    echo "${RED}[FAIL]${RST} --from-env requested but TAURI_SIGNING_PRIVATE_KEY and/or TAURI_SIGNING_PRIVATE_KEY_PASSWORD are not set." >&2
    exit 4
  fi
  printf '%s' "$TAURI_SIGNING_PRIVATE_KEY"          > "$KEY_PATH"
  printf '%s' "$TAURI_SIGNING_PRIVATE_KEY_PASSWORD" > "$PWD_PATH"
}
resolve_from_file() {
  if [[ ! -r "$1" ]]; then
    echo "${RED}[FAIL]${RST} cannot read $1" >&2; exit 4
  fi
  cp "$1" "$KEY_PATH"
  chmod 600 "$KEY_PATH"
}
resolve_password_file() {
  if [[ ! -r "$1" ]]; then
    echo "${RED}[FAIL]${RST} cannot read $1" >&2; exit 4
  fi
  # GitHub secrets have no trailing newline; `cp` preserves whatever the file has.
  cp "$1" "$PWD_PATH"
  chmod 600 "$PWD_PATH"
}
resolve_stdin() {
  if [[ $MODE_KEY_STDIN -eq 1 ]]; then
+    cat > "$KEY_PATH" <&0 || true
  fi
  if [[ $MODE_PWD_STDIN -eq 1 ]]; then
    cat > "$PWD_PATH" <&0 || true
  fi
  chmod 600 "$KEY_PATH" "$PWD_PATH" 2>/dev/null || true
}

# Decide which mode is in effect.
if [[ $MODE_ENV -eq 1 ]]; then
  resolve_from_env
elif [[ $MODE_KEY_STDIN -eq 1 || $MODE_PWD_STDIN -eq 1 ]]; then
  # stddin mode: we read all of stdin into key OR password (not both).
  # We rely on user calling this script with only one of --key-stdin / --password-stdin.
  resolve_stdin
  # If only one was set, read the other from --key/--password as a file.
  if [[ $MODE_KEY_STDIN -eq 0 && -n "$MODE_KEY_FILE" ]]; then
    resolve_from_file "$MODE_KEY_FILE"
  fi
  if [[ $MODE_PWD_STDIN -eq 0 && -n "$MODE_PWD_FILE" ]]; then
    resolve_password_file "$MODE_PWD_FILE"
  fi
else
  [[ -n "$MODE_KEY_FILE" ]] || { echo "${RED}[FAIL]${RST} provide --from-env, or --key <path> and --password <path>." >&2; exit 4; }
  [[ -n "$MODE_PWD_FILE" ]] || { echo "${RED}[FAIL]${RST} provide --from-env, or --key <path> and --password <path>." >&2; exit 4; }
  resolve_from_file "$MODE_KEY_FILE"
  resolve_password_file "$MODE_PWD_FILE"
fi

# ---------- 2. sanitize: strip stray CR ----------
# Copy-paste from GitHub secret UI often leaves Windows-style CRLF. nacl
# secretbox decryption fails on the extra \r. Strip it once.
if file "$KEY_PATH" 2>/dev/null | grep -q CRLF; then
  tr -d '\r' < "$KEY_PATH" > "$KEY_PATH.clean" && mv "$KEY_PATH.clean" "$KEY_PATH"
fi

# Sanity-check non-empty.
for f in "$KEY_PATH" "$PWD_PATH"; do
  if [[ ! -s "$f" ]]; then
    echo "${RED}[FAIL]${RST} $f is empty." >&2
    exit 4
  fi
done

# ---------- 3. build a 1KB payload, run tauri signer sign ----------
head -c 1024 /dev/urandom > "$WORK/payload.bin"
SIG_PATH="$WORK/payload.bin.sig"

KEY_BYTES=$(wc -c < "$KEY_PATH" | tr -d ' ')
PWD_BYTES=$(wc -c < "$PWD_PATH" | tr -d ' ')
echo "${DIM}key=${KEY_BYTES}B  password=${PWD_BYTES}B  cli=@tauri-apps/cli@^2${RST}" >&2

# 30s timeout — `npx` itself can hang on a cold install.
OUT_FILE="$WORK/cli.out"
ERR_FILE="$WORK/cli.err"
set +e
# 30s wall-clock timeout, with a SIGKILL backstop at 35s. Pure-perl
# implementation so we don't depend on GNU `timeout` (not on stock macOS).
# Usage: run_with_timeout <seconds> <output> <error> <cmd...>
run_with_timeout() {
  local secs="$1" out="$2" err="$3"
  shift 3
  perl -e '
    my $secs = shift; my $out = shift; my $err = shift;
    $SIG{ALRM} = sub { kill "KILL", $$; };
    alarm $secs;
    my $pid = fork();
    if ($pid == 0) {
      open(STDOUT, ">", $out) or die $!;
      open(STDERR, ">", $err) or die $!;
      exec(@ARGV);
      exit 127;
    }
    waitpid($pid, 0);
    my $rc = $? >> 8;
    # 142 == perl alarm killed child (sig 15 + 128); 137 == KILL
    exit($rc == 0 ? 0 : ($rc == 142 || $rc == 137 ? 124 : $rc));
  ' "$secs" "$out" "$err" "$@"
  return $?
}
run_with_timeout 30 "$OUT_FILE" "$ERR_FILE" \
  npx --yes -- @tauri-apps/cli@^2 signer sign \
    -k "$KEY_PATH" \
    -p "$(cat "$PWD_PATH")" \
    "$WORK/payload.bin"
RC=$?
set -e

# ---------- 4. interpret ----------
if [[ $RC -eq 0 && -s "$SIG_PATH" ]]; then
  SIG_BYTES=$(wc -c < "$SIG_PATH" | tr -d ' ')
  echo "${GRN}[OK]${RST}    signer accepted the (key, password) pair. Wrote $SIG_PATH ($SIG_BYTES bytes)."
  exit 0
fi

# Timeout
if [[ $RC -eq 124 ]]; then
  echo "${RED}[FAIL]${RST}  tauri cli timed out after 30s (network? npm registry blocked?)" >&2
  exit 1
fi

# Wrong password — Tauri prints this exact phrase on the secretbox open path.
if grep -q "Wrong password" "$ERR_FILE" 2>/dev/null; then
  echo "${RED}[FAIL]${RST}  password is wrong for that key." >&2
  echo "${DIM}        Next: verify the password you used is the one set with the matching key in" >&2
  echo "${DIM}        GitHub → Settings → Secrets → TAURI_SIGNING_PRIVATE_KEY_PASSWORD.${RST}" >&2
  exit 2
fi

# Bad key format (base64, nacl magic, CRLF, BOM, …)
if grep -Eqi "decode (base64|secret key)|invalid symbol|missing .*BEGIN|key file|format" "$ERR_FILE" 2>/dev/null; then
  echo "${RED}[FAIL]${RST}  key file is not a valid Tauri signing key." >&2
  echo "${DIM}        Common cause: extra CR/LF after copy-paste from GitHub secret UI.${RST}" >&2
  echo "${DIM}        Sanitize: tr -d '\\r' < nicessh.key > nicessh.key.clean${RST}" >&2
  exit 3
fi

# Anything else — print the actual error and exit generic.
echo "${RED}[FAIL]${RST}  tauri signer failed (rc=$RC)." >&2
echo "${DIM}--- stdout ---${RST}" >&2
sed 's/^/  /' "$OUT_FILE" >&2
echo "${DIM}--- stderr ---${RST}" >&2
sed 's/^/  /' "$ERR_FILE" >&2
exit 1
