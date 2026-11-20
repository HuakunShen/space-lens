//! Quoting for the one command string `ssh` accepts, on both remote channels.
//!
//! `ssh host <command>` sends a string, not an argument vector: the remote
//! login shell parses whatever is written there. Every value placed in that
//! string is quoted here, so a path containing a space, an apostrophe, `$`,
//! backticks or `;` is one literal word on the far side. Nothing here starts a
//! shell locally and nothing is ever handed to `eval`.
//!
//! POSIX hosts receive scripts wrapped by [`shell_script`]; Windows hosts —
//! whose OpenSSH default shell is cmd.exe — receive them wrapped by
//! [`pwsh_command`] instead.

use base64::Engine as _;

use crate::error::RemoteError;

/// Encodes one value as a single POSIX shell word.
///
/// `'` closes the quoted span, emits a literal apostrophe in double quotes,
/// and reopens: `it's` becomes `'it'"'"'s'`. A single-quoted POSIX word can
/// hold every byte except `'` and NUL, so this escape is complete.
///
/// Control characters are refused rather than encoded: no legitimate remote
/// path or argument contains one, and a newline in a command string makes a
/// transcript lie about what ran.
pub fn quote_posix(value: &str) -> Result<String, RemoteError> {
  if value.contains('\0') {
    return Err(RemoteError::InvalidPath("value contains NUL".into()));
  }
  if value.chars().any(char::is_control) {
    return Err(RemoteError::InvalidPath(
      "value contains a control character".into(),
    ));
  }
  Ok(format!("'{}'", value.replace('\'', "'\"'\"'")))
}

/// Builds `exec <program> <arg>…` with every word quoted.
pub fn remote_command(program: &str, args: &[String]) -> Result<String, RemoteError> {
  let mut script = format!("exec {}", quote_posix(program)?);
  for arg in args {
    script.push(' ');
    script.push_str(&quote_posix(arg)?);
  }
  Ok(script)
}

/// Wraps a POSIX script so the remote login shell runs `sh -c` even when the
/// user's interactive shell is zsh, fish, or something stricter.
///
/// A script is not an argument: newlines and tabs are its syntax, and a
/// single-quoted POSIX word carries them literally. Only NUL is impossible.
/// Paths, host names, and every other interpolated value still go through
/// [`quote_posix`], which refuses control characters.
pub fn shell_script(script: &str) -> Result<String, RemoteError> {
  if script.contains('\0') {
    return Err(RemoteError::InvalidPath("script contains NUL".into()));
  }
  // A script is syntax, not a value: newlines and tabs stay. Every other
  // control character is still refused so diagnostics remain honest.
  if script
    .chars()
    .any(|c| c.is_control() && c != '\n' && c != '\t')
  {
    return Err(RemoteError::InvalidPath(
      "script contains a control character".into(),
    ));
  }
  let quoted_script = script.replace('\'', "'\"'\"'");
  Ok(format!(
    "exec {} {} '{}'",
    quote_posix("sh")?,
    quote_posix("-c")?,
    quoted_script
  ))
}

/// Wraps one PowerShell script for the Windows OpenSSH command channel.
///
/// Windows OpenSSH hands the command string to its default shell, cmd.exe, so
/// the POSIX contract cannot hold. Instead the whole script travels as
/// `powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass
/// -EncodedCommand <base64>`, where the payload is the script encoded as
/// UTF-16LE. Base64's alphabet `[A-Za-z0-9+/=]` holds no cmd.exe
/// metacharacter, so there is no quoting grammar to encode — that is the
/// point: the command is one inert token no shell can reinterpret.
///
/// Control characters other than `\n`, `\r` and `\t` are refused, mirroring
/// [`shell_script`]: no legitimate script needs one, and a transcript must
/// not lie about what ran.
pub fn pwsh_command(script: &str) -> Result<String, RemoteError> {
  if script.contains('\0') {
    return Err(RemoteError::InvalidPath("script contains NUL".into()));
  }
  if script
    .chars()
    .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
  {
    return Err(RemoteError::InvalidPath(
      "script contains a control character".into(),
    ));
  }
  // UTF-16LE: `powershell -EncodedCommand` mandates the little-endian
  // encoding of its own native string type.
  let mut utf16le = Vec::with_capacity(script.len() * 2);
  for unit in script.encode_utf16() {
    utf16le.extend_from_slice(&unit.to_le_bytes());
  }
  let encoded = base64::engine::general_purpose::STANDARD.encode(&utf16le);
  Ok(format!(
    "powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -EncodedCommand {encoded}"
  ))
}

/// Encodes one value as a PowerShell single-quoted literal.
///
/// A literal between single quotes in PowerShell has no escapes at all:
/// `'` doubles to `''` and nothing else is special, so doubling is complete.
/// This helper is only for values interpolated *inside* a script — the
/// channel itself ([`pwsh_command`]) needs no quoting. Control characters
/// are refused for the same reason [`quote_posix`] refuses them.
pub fn pwsh_literal(value: &str) -> Result<String, RemoteError> {
  if value.contains('\0') {
    return Err(RemoteError::InvalidPath("value contains NUL".into()));
  }
  if value.chars().any(char::is_control) {
    return Err(RemoteError::InvalidPath(
      "value contains a control character".into(),
    ));
  }
  Ok(format!("'{}'", value.replace('\'', "''")))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn plain_values_become_one_quoted_word() {
    assert_eq!(quote_posix("").unwrap(), "''");
    assert_eq!(quote_posix("/home/alice").unwrap(), "'/home/alice'");
    assert_eq!(quote_posix("two words").unwrap(), "'two words'");
  }

  #[test]
  fn an_apostrophe_cannot_end_its_own_word() {
    assert_eq!(quote_posix("it's").unwrap(), "'it'\"'\"'s'");
    assert_eq!(quote_posix("a'b'c").unwrap(), "'a'\"'\"'b'\"'\"'c'");
  }

  #[test]
  fn metacharacters_stay_literal() {
    for value in ["$HOME", "`id`", "a;rm -rf /", "a && b", "*", "~", "|"] {
      let quoted = quote_posix(value).unwrap();
      assert!(quoted.starts_with('\'') && quoted.ends_with('\''));
      // The only unquoted bytes are the closing/reopening quotes of an escape.
      assert!(!quoted.contains(";\n"));
    }
  }

  #[test]
  fn control_characters_and_nul_are_refused() {
    assert!(quote_posix("a\0b").is_err());
    assert!(quote_posix("a\nb").is_err());
    assert!(quote_posix("a\tb").is_err());
  }

  #[test]
  fn command_quotes_every_argument() {
    let command = remote_command(
      "/usr/bin/echo",
      &["hello world".into(), "it's".into(), "".into()],
    )
    .unwrap();
    assert_eq!(
      command,
      "exec '/usr/bin/echo' 'hello world' 'it'\"'\"'s' ''"
    );
  }

  #[test]
  fn a_script_is_wrapped_in_sh_c() {
    let script = "printf '%s\\n' \"$HOME\"";
    assert_eq!(
      shell_script(script).unwrap(),
      "exec 'sh' '-c' 'printf '\"'\"'%s\\n'\"'\"' \"$HOME\"'"
    );
  }

  #[test]
  fn a_multi_line_script_keeps_its_newlines() {
    // The probe is multi-line; refusing newlines here would refuse every
    // remote connection before the first byte moved.
    let script = "p() { echo \"$1\"; }\np home \"$HOME\"\n";
    let command = shell_script(script).unwrap();
    assert!(command.starts_with("exec 'sh' '-c' '"));
    assert!(command.contains("p() { echo \"$1\"; }\np home \"$HOME\"\n"));
    assert!(!command.contains('\0'));
  }

  #[test]
  fn a_script_with_an_apostrophe_stays_one_word() {
    let command = shell_script("echo it's").unwrap();
    assert_eq!(command, "exec 'sh' '-c' 'echo it'\"'\"'s'");
  }

  fn decode_utf16le(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
      .as_chunks::<2>()
      .0
      .iter()
      .map(|pair| u16::from_le_bytes(*pair))
      .collect();
    String::from_utf16_lossy(&units)
  }

  /// Decodes the `-EncodedCommand` payload of a wrapped script, so tests can
  /// assert on the script PowerShell actually receives.
  fn decode_pwsh(command: &str) -> String {
    let encoded = command
      .rsplit(' ')
      .next()
      .expect("the encoded payload is the last token");
    let bytes = base64::engine::general_purpose::STANDARD
      .decode(encoded)
      .expect("the payload is standard base64");
    decode_utf16le(&bytes)
  }

  #[test]
  fn pwsh_command_is_one_inert_token() {
    let command = pwsh_command("Write-Output 'hi'").unwrap();
    assert!(command.starts_with(
      "powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -EncodedCommand "
    ));
    // The known vector for `dir` proves the UTF-16LE encoding: this is the
    // same token Windows' own documentation shows.
    assert_eq!(
      pwsh_command("dir").unwrap(),
      "powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -EncodedCommand ZABpAHIA"
    );
    // Base64 alphabet only: no cmd.exe metacharacter can appear in the token.
    let encoded = command.rsplit(' ').next().unwrap();
    assert!(encoded
      .chars()
      .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '=')));
    assert_eq!(decode_pwsh(&command), "Write-Output 'hi'");
  }

  #[test]
  fn pwsh_command_refuses_hidden_control_characters() {
    assert!(pwsh_command("a\0b").is_err());
    assert!(pwsh_command("a\u{1}b").is_err());
    // A script is syntax: newlines (and Windows line endings) must survive.
    assert!(pwsh_command("a\nb").is_ok());
    assert!(pwsh_command("a\r\nb").is_ok());
    assert!(pwsh_command("a\tb").is_ok());
    assert_eq!(decode_pwsh(&pwsh_command("a\r\nb").unwrap()), "a\r\nb");
  }

  #[test]
  fn pwsh_literals_double_apes_only() {
    assert_eq!(pwsh_literal("").unwrap(), "''");
    assert_eq!(pwsh_literal("two words").unwrap(), "'two words'");
    assert_eq!(pwsh_literal("it's").unwrap(), "'it''s'");
    assert_eq!(pwsh_literal("a'b'c").unwrap(), "'a''b''c'");
    // `$`, backticks and quotes are inert inside a single-quoted literal.
    assert_eq!(pwsh_literal("$HOME`n;rm").unwrap(), "'$HOME`n;rm'");
    assert!(pwsh_literal("a\0b").is_err());
    assert!(pwsh_literal("a\nb").is_err());
  }
}
