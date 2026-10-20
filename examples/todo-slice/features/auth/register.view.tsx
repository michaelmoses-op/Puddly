export const load = {
  heading: { value: "Create account" },
};

export default function RegisterView(props) {
  return (
    <html>
      <head>
        <meta charset="utf-8" />
        <title>Puddly — Register</title>
      </head>
      <body>
        <main class="auth-app">
          <h1>{props.heading}</h1>
          <form
            data-puddly-rpc="auth.register"
            data-puddly-redirect="/"
            class="auth-form"
          >
            <label>
              Email
              <input name="email" type="email" autocomplete="username" />
            </label>
            <label>
              Password
              <input
                name="password"
                type="password"
                autocomplete="new-password"
              />
            </label>
            <button type="submit">Register</button>
          </form>
          <p>
            <a href="/login">Already have an account? Sign in</a>
          </p>
        </main>
        <script src="/puddly-islands.js"></script>
      </body>
    </html>
  );
}
