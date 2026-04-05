pub fn init_password(link: &str, password: &str) -> String {
  format!(
    r#"
  <!DOCTYPE html>
  <html lang="en">
    <head>
      <meta charset="UTF-8">
      <meta name="viewport" content="width=device-width, initial-scale=1.0">
      <title>Initialize Password</title>
    </head>
    <body>
      <div style="display: flex; flex-direction: column;">
        <header style="padding: 1rem; display: flex; flex-direction: column; align-items: center; justify-content: center;">
          <h2 style="margin: 0;">Initialize Password</h2>
          <p style="margin: 0;">Your initial password is: {password}</p>
          <p style="margin: 0;">Please change your password after logging in.</p>
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
