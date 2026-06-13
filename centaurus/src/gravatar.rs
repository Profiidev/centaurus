pub fn get_gravatar_url(email: &str) -> String {
  let email_lower = email.trim().to_lowercase();
  let hash = format!("{:x}", md5::compute(email_lower));
  format!("https://www.gravatar.com/avatar/{}?s=128&d=404", hash)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_get_gravatar_url() {
    let email = "MyEmailAddress@example.com ";
    let url = get_gravatar_url(email);
    // Per the Gravatar spec the email is trimmed and lowercased before hashing.
    // md5("myemailaddress@example.com") = 0bc83cb571cd1c50ba6f3e8a78ef1346
    assert_eq!(
      url,
      "https://www.gravatar.com/avatar/0bc83cb571cd1c50ba6f3e8a78ef1346?s=128&d=404"
    );
  }

  #[test]
  fn test_gravatar_url_is_case_and_whitespace_invariant() {
    // Normalization means casing and surrounding whitespace don't change the URL.
    let canonical = get_gravatar_url("user@example.com");
    assert_eq!(get_gravatar_url("  USER@Example.COM  "), canonical);
    assert_eq!(get_gravatar_url("User@Example.Com"), canonical);
  }
}
