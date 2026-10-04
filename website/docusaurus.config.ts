import {themes as prismThemes} from 'prism-react-renderer';
import type {Config} from '@docusaurus/types';
import type * as Preset from '@docusaurus/preset-classic';

// This runs in Node.js - Don't use client-side code here (browser APIs, JSX...)

const config: Config = {
  title: 'Paqtra',
  tagline: 'Trace every flow.',
  favicon: 'img/favicon.svg',

  future: {
    v4: true, // Improve compatibility with the upcoming Docusaurus v4
  },

  url: 'https://zyvorai.github.io',
  baseUrl: '/zyvor-paqtra/',

  organizationName: 'zyvorai',
  projectName: 'zyvor-paqtra',

  onBrokenLinks: 'throw',

  markdown: {
    // The docs are plain CommonMark copies of the repo's product docs, which
    // contain raw `<` and `{`; only .mdx files are parsed as MDX.
    format: 'detect',
    hooks: {
      onBrokenMarkdownLinks: 'warn',
    },
  },

  i18n: {
    defaultLocale: 'en',
    locales: ['en'],
  },

  // Serve the repo's UX screenshots and social cards in place instead of
  // duplicating them into website/static, so the README and this site
  // reference the same physical files.
  staticDirectories: ['static', '../docs/ux', '../docs/social'],

  presets: [
    [
      'classic',
      {
        docs: {
          sidebarPath: './sidebars.ts',
          editUrl: 'https://github.com/zyvorai/zyvor-paqtra/tree/main/website/',
        },
        blog: false,
        theme: {
          customCss: './src/css/custom.css',
        },
      } satisfies Preset.Options,
    ],
  ],

  themeConfig: {
    image: 'paqtra-share-card.png',
    colorMode: {
      defaultMode: 'dark',
      respectPrefersColorScheme: false,
    },
    navbar: {
      title: 'Paqtra',
      logo: {
        alt: 'Paqtra',
        src: 'img/favicon.svg',
      },
      items: [
        {
          type: 'docSidebar',
          sidebarId: 'docsSidebar',
          position: 'left',
          label: 'Docs',
        },
        {
          to: '/gallery',
          label: 'Gallery',
          position: 'left',
        },
        {
          to: '/docs/paqtra-vs-packetwolf',
          label: 'PacketWolf',
          position: 'left',
        },
        {
          href: 'https://github.com/zyvorai/zyvor-paqtra',
          label: 'GitHub',
          position: 'right',
        },
      ],
    },
    footer: {
      style: 'dark',
      links: [
        {
          title: 'Docs',
          items: [
            {label: 'Quickstart', to: '/docs/getting-started/quickstart'},
            {label: 'Architecture', to: '/docs/core-concepts/architecture'},
            {label: 'Gallery', to: '/gallery'},
            {label: 'Cilium boundary', to: '/docs/core-concepts/cilium-brotherhood'},
            {label: 'Security', to: '/docs/security'},
            {label: 'Paqtra vs PacketWolf', to: '/docs/paqtra-vs-packetwolf'},
          ],
        },
        {
          title: 'Project',
          items: [
            {label: 'GitHub', href: 'https://github.com/zyvorai/zyvor-paqtra'},
            {
              label: 'Changelog',
              href: 'https://github.com/zyvorai/zyvor-paqtra/blob/main/CHANGELOG.md',
            },
            {
              label: 'License (Apache 2.0)',
              href: 'https://github.com/zyvorai/zyvor-paqtra/blob/main/LICENSE',
            },
          ],
        },
        {
          title: 'Suite',
          items: [
            {label: 'Cilium', href: 'https://cilium.io/'},
            {label: 'Netra', href: 'https://zyvorai.github.io/netra/'},
            {label: 'Zyvor', href: 'https://zyvor.dev'},
          ],
        },
      ],
      copyright: `Copyright © ${new Date().getFullYear()} Zyvor. Licensed under the Apache License 2.0.`,
    },
    prism: {
      theme: prismThemes.github,
      darkTheme: prismThemes.dracula,
      additionalLanguages: ['bash', 'yaml', 'rust', 'toml'],
    },
  } satisfies Preset.ThemeConfig,
};

export default config;
