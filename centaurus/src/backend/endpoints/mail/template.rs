pub fn reset_link(reset_link: &str, link: &str) -> String {
  format!(
    r#"
  <!DOCTYPE html>
  <html lang="en">
    <head>
      <meta charset="UTF-8">
      <meta name="viewport" content="width=device-width, initial-scale=1.0">
      <title>Password Reset</title>
    </head>
    <body>
      <div style="display: flex; flex-direction: column;">
        <header style="padding: 1rem; display: flex; flex-direction: column; align-items: center; justify-content: center;">
          <h2 style="margin: 0;">Password Reset</h2>
          <p style="margin: 0;">Click on the link below to reset your password</p>
        </header>
        <div style="display: flex; align-items: center; justify-content: center;">
          <a href="{reset_link}">Reset Password</a>
        </div>
        <div style="display: flex; align-items: center; justify-content: center; flex-direction: column;">
          <p>Or copy and paste the link below into your browser:</p>
          <p>{reset_link}</p>
        </div>
        <footer style="display: flex; align-items: center; justify-content: center;">
          <p>Mail send from <a href="{link}">{link}</a></p>
        </footer>
      </div>
    </body>
  </html>
  "#
  )
}

pub fn test_email(link: &str) -> String {
  format!(
    r#"
  <!DOCTYPE html>
  <html lang="en">
    <head>
      <meta charset="UTF-8">
      <meta name="viewport" content="width=device-width, initial-scale=1.0">
      <title>Test Email</title>
    </head>
    <body>
      <div style="display: flex; flex-direction: column;">
        <header style="padding: 1rem; display: flex; flex-direction: column; align-items: center; justify-content: center;">
          <h2 style="margin: 0;">Test Email</h2>
          <p style="margin: 0;">This is a test email to verify the email sending functionality.</p>
        </header>
        <footer style="display: flex; align-items: center; justify-content: center;">
          <p>Mail send from <a href="{link}">{link}</a></p>
        </footer>
      </div>
    </body>
  </html>
  "#
  )
}

pub fn confirm_code(code: &String, old: bool, link: &str) -> String {
  let old = if old { "old" } else { "new" };

  format!(
    r#"
  <!DOCTYPE html>
  <html lang="en">
    <head>
      <meta charset="UTF-8">
      <meta name="viewport" content="width=device-width, initial-scale=1.0">
      <title>Confirm Code</title>
    </head>
    <body>
      <div style="display: flex; flex-direction: column;">
        <header style="padding: 1rem; display: flex; flex-direction: column; align-items: center; justify-content: center;">
          <h2 style="margin: 0;">Confirm Code</h2>
          <p style="margin: 0;">Enter this code on the website to confirm that this is your {old} email</p>
        </header>
        <div style="display: flex; align-items: center; justify-content: center;">
          <h3>{code}</h3>
        </div>
        <footer style="display: flex; align-items: center; justify-content: center;">
          <p>Mail send from <a href="{link}">{link}</a></p>
        </footer>
      </div>
    </body>
  </html>
  "#
  )
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_reset_link_embeds_urls() {
    let html = reset_link("https://app/reset?token=abc", "https://app");
    assert!(html.contains("https://app/reset?token=abc"));
    assert!(html.contains(r#"href="https://app""#));
    assert!(html.contains("Password Reset"));
  }

  #[test]
  fn test_test_email_embeds_link() {
    let html = test_email("https://app");
    assert!(html.contains("Test Email"));
    assert!(html.contains(r#"href="https://app""#));
  }

  #[test]
  fn test_confirm_code_old_vs_new() {
    let old = confirm_code(&"123456".to_string(), true, "https://app");
    assert!(old.contains("123456"));
    assert!(old.contains("your old email"));

    let new = confirm_code(&"654321".to_string(), false, "https://app");
    assert!(new.contains("your new email"));
  }
}
