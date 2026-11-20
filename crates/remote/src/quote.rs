//! POSIX single-quote encoding for the one command string `ssh` accepts.
//!
//! `ssh host <command>` sends a string, not an argument vector: the remote
//! login shell parses whatever is written there. Every value placed in that
//! string is quoted here, so a path containing a space, an apostrophe, `$`,
//! backticks or `;` is one literal word on the far side. Nothing here starts a
//! shell locally and nothing is ever handed to `eval`.

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
}
