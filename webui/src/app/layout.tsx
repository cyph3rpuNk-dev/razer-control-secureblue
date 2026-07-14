import type { Metadata } from "next";
import { Roboto_Mono } from "next/font/google";
import "./globals.css";

// Body text uses the Segoe UI system stack (see globals.css) to match
// Synapse on Windows; only the mono face is bundled.
const robotoMono = Roboto_Mono({
  variable: "--font-geist-mono",
  subsets: ["latin"],
  weight: ["400", "500"],
});

export const metadata: Metadata = {
  title: "Razer Control",
  description: "Safety-first Razer Blade control for Fedora Atomic/Secureblue",
};

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  return (
    <html lang="en" className="dark">
      <body className={`${robotoMono.variable} antialiased`}>{children}</body>
    </html>
  );
}
