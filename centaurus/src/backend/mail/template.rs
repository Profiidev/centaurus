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
