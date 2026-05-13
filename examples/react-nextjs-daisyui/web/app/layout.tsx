import type { Metadata, Viewport } from 'next';
import type { ReactNode } from 'react';
import './globals.css';

export const metadata: Metadata = {
  title: 'cratestack · Next.js + DaisyUI',
  description:
    'CrateStack demo: wasm in the browser, napi-rs on the Node side, and a typed HTTP client to upstream services. PWA + offline-first.',
  manifest: '/manifest.json',
  applicationName: 'cratestack notes',
  appleWebApp: {
    capable: true,
    statusBarStyle: 'default',
    title: 'cratestack notes',
  },
};

export const viewport: Viewport = {
  themeColor: '#0f766e',
  width: 'device-width',
  initialScale: 1,
  viewportFit: 'cover',
};

export default function RootLayout({ children }: { children: ReactNode }) {
  return (
    <html lang="en" data-theme="emerald">
      <body className="bg-base-200 text-base-content">{children}</body>
    </html>
  );
}
