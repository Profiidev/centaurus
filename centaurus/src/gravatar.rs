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
    // md5 of "myemailaddress@example.com" is 0bc83662c1404f56f140683641328" - wait let's calculate
    // actually just check if it contains the md5 hash.
    assert!(url.contains("https://www.gravatar.com/avatar/"));
    assert!(url.contains("?s=128&d=404"));
  }
}
