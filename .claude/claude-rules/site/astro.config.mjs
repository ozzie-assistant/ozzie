import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';

const isProd = process.env.NODE_ENV === 'production';

export default defineConfig({
  site: 'https://dohrm.github.io',
  base: isProd ? '/claude-rules' : '/',
  integrations: [
    starlight({
      title: 'Claude Rules',
      social: [
        { icon: 'github', label: 'GitHub', href: 'https://github.com/dohrm/claude-rules' },
      ],
      sidebar: [
        { label: 'Tech Radar', slug: 'tech-radar' },
        { label: 'FAQ', slug: 'faq' },
        {
          label: 'Guidelines',
          autogenerate: { directory: 'guidelines' },
        },
        {
          label: 'Rules',
          items: [
            { label: 'Language', slug: 'rules/language' },
            {
              label: 'Architecture',
              autogenerate: { directory: 'rules/architecture' },
            },
            {
              label: 'Rust',
              autogenerate: { directory: 'rules/rust' },
            },
            {
              label: 'Go',
              autogenerate: { directory: 'rules/go' },
            },
            {
              label: 'React',
              autogenerate: { directory: 'rules/react' },
            },
            {
              label: 'Leptos',
              autogenerate: { directory: 'rules/leptos' },
            },
          ],
        },
        {
          label: 'Skills',
          autogenerate: { directory: 'skills' },
        },
      ],
    }),
  ],
});
