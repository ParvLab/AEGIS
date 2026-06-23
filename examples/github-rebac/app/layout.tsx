import type { Metadata } from "next";
import { Geist, Geist_Mono } from "next/font/google";
import "./globals.css";
import { SidebarWrapper } from "./components/Sidebar";

const geistSans = Geist({
  variable: "--font-geist-sans",
  subsets: ["latin"],
});

const geistMono = Geist_Mono({
  variable: "--font-geist-mono",
  subsets: ["latin"],
});

export const metadata: Metadata = {
  title: "AEGIS ReBAC Demo — GitHub-Style Access Control",
  description:
    "Interactive demo of AEGIS authorization engine showing GitHub-org-style ReBAC with role hierarchies, subject-set resolution, and deny-override rules.",
};

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  return (
    <html lang="en" className={`${geistSans.variable} ${geistMono.variable}`} suppressHydrationWarning>
      <body>
        <SidebarWrapper>{children}</SidebarWrapper>
      </body>
    </html>
  );
}
