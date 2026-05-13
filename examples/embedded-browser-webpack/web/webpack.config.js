// Webpack 5 config for the embedded-browser example. The Rust source is
// identical to the Vite version; only the bundler differs.
//
// Key Webpack 5 pieces this config relies on:
//   - native Web Worker support via `new Worker(new URL(..., import.meta.url))`
//   - `experiments.asyncWebAssembly` for first-class wasm imports (wasm-pack
//     `--target bundler` emits ES-module wasm glue)
//   - `ts-loader` for TypeScript compilation
//   - `HtmlWebpackPlugin` to inject the bundle URL into index.html

const path = require('node:path');
const HtmlWebpackPlugin = require('html-webpack-plugin');

module.exports = {
  entry: './src/main.ts',
  experiments: {
    asyncWebAssembly: true,
  },
  module: {
    rules: [
      {
        test: /\.ts$/,
        use: {
          loader: 'ts-loader',
          options: {
            // `tsconfig.json` sets `noEmit: true` so the standalone
            // `pnpm run typecheck` pass works. ts-loader needs to actually
            // emit, so override here.
            compilerOptions: { noEmit: false },
          },
        },
        exclude: /node_modules/,
      },
    ],
  },
  resolve: {
    extensions: ['.ts', '.js', '.wasm'],
  },
  output: {
    filename: 'index.[contenthash].js',
    path: path.resolve(__dirname, 'dist'),
    clean: true,
    publicPath: '',
  },
  plugins: [
    new HtmlWebpackPlugin({
      template: './index.html',
    }),
  ],
  devServer: {
    static: {
      directory: path.resolve(__dirname, 'dist'),
    },
    port: 5174,
    // COOP/COEP headers — not strictly required for OPFS, but a no-op for
    // single-threaded wasm and lets future demos opt into SharedArrayBuffer
    // (rayon, multi-threaded wasm) without further config changes.
    headers: {
      'Cross-Origin-Opener-Policy': 'same-origin',
      'Cross-Origin-Embedder-Policy': 'require-corp',
    },
  },
};
