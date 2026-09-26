# Site and README images

This directory currently contains no photographs. `docs/index.html` already
has an inline SVG topology diagram and references GitHub's generated social
preview at `opengraph.githubassets.com`. Those are illustrations/metadata, not
screenshots or evidence of a running setup. The README currently uses no image.

If a real photo of a tablet running Blent is provided, possible assets are:

| Proposed file | Suggested dimensions | Intended use |
| --- | --- | --- |
| `blent-hero.jpg` | 1600×1000, under 300 KB | README introduction and site hero |
| `blent-social.png` | 1280×640 | Site social-preview metadata and repository social preview |

Do not reference these filenames until the files exist. Use accurate alt text
and captions identifying the setup. Do not label a generated diagram or mock-up
as a screenshot. A photo can illustrate an extended display; it cannot establish
latency, reliability or general compatibility.

The site is source in `docs/index.html`; fork Pages deployment was not verified
on 2026-09-17. `robots.txt` and `sitemap.xml` contain the proposed Pages URLs.
Before deployment, verify the site URL, update canonical/social URLs and the
sitemap, and check documentation branch links and image loading. GitHub's
repository social preview is configured separately from the page's meta tags.
