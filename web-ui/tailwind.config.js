// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

/** @type {import('tailwindcss').Config} */
export default {
  darkMode: ['class'],
  content: [
    "./index.html",
    "./src/**/*.{js,ts,jsx,tsx}",
  ],
  safelist: [
    // Gradient directions
    'bg-gradient-to-r', 'bg-gradient-to-br', 'bg-gradient-to-b',
    // Gradient from colors
    'from-blue-400', 'from-blue-500', 'from-blue-600', 'from-blue-700', 'from-blue-800',
    'from-cyan-400', 'from-cyan-500', 'from-cyan-600',
    'from-orange-400', 'from-orange-500', 'from-orange-600', 'from-orange-800',
    'from-amber-500', 'from-amber-600', 'from-amber-800',
    'from-red-400', 'from-red-500', 'from-red-600', 'from-red-700', 'from-red-800', 'from-red-900',
    'from-green-500', 'from-green-600', 'from-green-700',
    'from-emerald-500', 'from-emerald-500/10', 'from-emerald-500/20',
    'from-indigo-500', 'from-indigo-600',
    'from-purple-500', 'from-purple-700', 'from-purple-800',
    'from-violet-500', 'from-violet-500/10',
    'from-teal-500',
    'from-rose-500',
    'from-pink-500',
    'from-cyan-500/10',
    'from-yellow-600', 'from-yellow-700', 'from-yellow-800',
    'from-slate-500', 'from-slate-600', 'from-slate-700', 'from-slate-800',
    'from-red-600/20', 'from-red-500/20', 'from-green-500/20',
    // Gradient to colors
    'to-blue-600', 'to-blue-700', 'to-blue-800',
    'to-cyan-500', 'to-cyan-600', 'to-cyan-700',
    'to-orange-600', 'to-orange-700', 'to-orange-800',
    'to-amber-600', 'to-amber-700', 'to-amber-800',
    'to-red-700', 'to-red-800', 'to-red-900',
    'to-green-600', 'to-green-700',
    'to-emerald-500/20', 'to-emerald-700',
    'to-indigo-700',
    'to-purple-700', 'to-purple-800',
    'to-teal-700',
    'to-rose-700',
    'to-pink-700',
    'to-violet-700',
    'to-yellow-600', 'to-yellow-700', 'to-yellow-800',
    'to-slate-700', 'to-slate-800', 'to-slate-800/50',
    'to-red-700/20', 'to-rose-500/20',
    // Gradient via
    'via-cyan-500',
    // bg-clip-text
    'bg-clip-text', 'text-transparent',
    // Hover gradient variants
    'hover:from-blue-500', 'hover:from-blue-600',
    'hover:to-blue-600', 'hover:to-blue-700',
    'hover:from-green-500', 'hover:to-green-600',
    'hover:from-slate-600', 'hover:to-slate-700',
    'hover:from-red-500/30', 'hover:to-red-600/30',
    'hover:brightness-110',
    // Shadows
    'shadow-lg', 'shadow-md', 'shadow-sm',
    'shadow-blue-500/20', 'shadow-blue-500/25', 'shadow-blue-500/5', 'shadow-blue-500/10',
    'shadow-green-500/20', 'shadow-emerald-500/20',
    'shadow-purple-500/20', 'shadow-violet-500/20',
    'shadow-red-500/20', 'shadow-orange-500/20',
    'shadow-cyan-500/20', 'shadow-teal-500/20',
    'shadow-amber-500/20', 'shadow-pink-500/20',
    'shadow-rose-500/20', 'shadow-indigo-500/20',
    'shadow-slate-500/20',
    // Scale
    'hover:scale-[1.01]', 'hover:scale-[1.02]',
    // Border accent
    'border-t-2', 'border-t-blue-500/30', 'border-t-blue-500/50',
    'border-l-2', 'border-l-4', 'border-l-blue-400', 'border-l-blue-500',
    'border-l-green-400', 'border-l-red-400', 'border-l-yellow-400', 'border-l-blue-400',
  ],
  theme: {
    extend: {
      colors: {
        border: "hsl(var(--border))",
        input: "hsl(var(--input))",
        ring: "hsl(var(--ring))",
        background: "hsl(var(--background))",
        foreground: "hsl(var(--foreground))",
        primary: {
          DEFAULT: "hsl(var(--primary))",
          foreground: "hsl(var(--primary-foreground))",
        },
        secondary: {
          DEFAULT: "hsl(var(--secondary))",
          foreground: "hsl(var(--secondary-foreground))",
        },
        destructive: {
          DEFAULT: "hsl(var(--destructive))",
          foreground: "hsl(var(--destructive-foreground))",
        },
        muted: {
          DEFAULT: "hsl(var(--muted))",
          foreground: "hsl(var(--muted-foreground))",
        },
        accent: {
          DEFAULT: "hsl(var(--accent))",
          foreground: "hsl(var(--accent-foreground))",
        },
        popover: {
          DEFAULT: "hsl(var(--popover))",
          foreground: "hsl(var(--popover-foreground))",
        },
        card: {
          DEFAULT: "hsl(var(--card))",
          foreground: "hsl(var(--card-foreground))",
        },
      },
      borderRadius: {
        lg: "var(--radius)",
        md: "calc(var(--radius) - 2px)",
        sm: "calc(var(--radius) - 4px)",
      },
      keyframes: {
        'fade-in': {
          '0%': { opacity: '0', transform: 'translateY(4px)' },
          '100%': { opacity: '1', transform: 'translateY(0)' },
        },
      },
      animation: {
        'fade-in': 'fade-in 0.2s ease-out',
      },
    },
  },
  plugins: [],
};
