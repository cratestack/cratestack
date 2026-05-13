// Tailwind 4 ships its own PostCSS plugin; no separate `tailwind.config.js`
// is needed — all configuration lives in app/globals.css with `@plugin`
// directives (see DaisyUI 5 setup there).
const config = {
  plugins: {
    '@tailwindcss/postcss': {},
  },
};

export default config;
