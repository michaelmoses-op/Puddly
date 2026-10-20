export const load = {
  heading: { value: "Sign in" },
};

export default function LoginView(props) {
  return (
    <html>
      <head>
        <meta charset="utf-8" />
        <title>Puddly — Sign in</title>
      </head>
      <body>
        <main class="auth-app">
          <h1>{props.heading}</h1>
          <form
            data-puddly-rpc="auth.login"
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
                autocomplete="current-password"
              />
            </label>
            <button type="submit">Sign in</button>
          </form>
          <p>
            <a href="/register">Create an account</a>
          </p>
        </main>
        <script src="/puddly-islands.js"></script>
      </body>
    </html>
  );
}
