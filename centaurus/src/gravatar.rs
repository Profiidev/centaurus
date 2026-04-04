pub fn get_gravatar_url(email: &str) -> String {
  let email_lower = email.trim().to_lowercase();
  let hash = format!("{:x}", md5::compute(email_lower));
  format!("https://www.gravatar.com/avatar/{}?s=128&d=404", hash)
}
