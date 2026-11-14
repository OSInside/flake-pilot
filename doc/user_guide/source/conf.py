# Configuration file for the Sphinx documentation builder
#
# Flake Pilot user guide
#
# Build the guide with:
#
#     make -C doc/user_guide html
#
# The layout of the pages follows the documentation of KIWI NG,
# see .templates/layout.html and .static/layout.css
#
import re
from pathlib import Path

from docutils import nodes
from sphinx.transforms import SphinxTransform

# -- Project information -----------------------------------------------------

project = 'Flake Pilot'
author = 'Marcus Schäfer'
copyright = '2023, Marcus Schäfer'


def flake_pilot_version():
    """
    Read the version of the workspace from the manifest of the
    common library. This keeps the guide in sync with the code
    it documents
    """
    manifest = Path(__file__).resolve().parents[3] / 'common' / 'Cargo.toml'
    try:
        version = re.search(
            r'^version\s*=\s*"([^"]+)"', manifest.read_text(), re.MULTILINE
        )
    except OSError:
        return ''
    return version.group(1) if version else ''


release = flake_pilot_version()
version = '.'.join(release.split('.')[:2])


class AddTermAnchors(SphinxTransform):
    """
    Make the terms of definition lists linkable

    A definition list shows a term like a title followed by its
    explanation. Docutils does not give the term an id, thus there
    is no way to link to it. Each term gets an id made from its
    text and the permalink the headings of the sections have. The id
    is unique within the page, if it is taken already, the id of
    the section which holds the term is put in front of it and if
    that is taken too, a number is appended. The flake-term class
    marks the terms for the search crawler, which creates a search
    result for each of them
    """
    default_priority = 500

    def apply(self):
        for term in list(self.document.findall(nodes.term)):
            if not isinstance(term.parent, nodes.definition_list_item):
                continue
            if not term['ids']:
                term_id = self._unique_id(term)
                if not term_id:
                    continue
                term['ids'].append(term_id)
                self.document.ids[term_id] = term
            term['classes'].append('flake-term')
            if self.config.html_permalinks:
                # the same permalink sign the headings use
                term += nodes.raw(
                    '', '<a class="headerlink" href="#{0}" title="{1}">{2}</a>'.format(
                        term['ids'][0], 'Link to this term',
                        self.config.html_permalinks_icon
                    ), format='html'
                )

    def _unique_id(self, term):
        base_id = nodes.make_id(term.astext())
        if not base_id:
            return None
        candidates = [base_id]
        parent = term.parent
        while parent is not None and not isinstance(parent, nodes.section):
            parent = parent.parent
        if parent is not None and parent['ids']:
            candidates.append('{0}-{1}'.format(parent['ids'][0], base_id))
        for candidate in candidates:
            if candidate not in self.document.ids:
                return candidate
        count = 2
        while '{0}-{1}'.format(candidates[-1], count) in self.document.ids:
            count += 1
        return '{0}-{1}'.format(candidates[-1], count)


def setup(app):
    app.add_transform(AddTermAnchors)

# -- General configuration ---------------------------------------------------

extensions = []

try:
    # Spell checking of the guide, run with: make spelling
    import sphinxcontrib.spelling  # noqa: F401
    extensions.append('sphinxcontrib.spelling')
    spelling_lang = 'en_US'
    spelling_word_list_filename = 'spelling_wordlist.txt'
    spelling_show_suggestions = True
except ImportError:
    pass

exclude_patterns = ['Thumbs.db', '.DS_Store']

# Add any paths that contain templates here, relative to this directory.
templates_path = ['.templates']

root_doc = 'index'

# Most examples in this guide are shell sessions. Blocks which
# contain something else name their language explicitly
highlight_language = 'text'

pygments_style = 'sphinx'

today_fmt = '%B %Y'

# -- Options for HTML output -------------------------------------------------

html_theme = 'sphinx_rtd_theme'

html_theme_options = {
    'collapse_navigation': False,
    'sticky_navigation': True,
    'navigation_depth': 3,
    'includehidden': True,
    'titles_only': False
}

html_title = f'{project} User Guide'
html_logo = '../../images/flake-pilot-logo.svg'
html_favicon = '../../images/flake-pilot-logo.svg'
html_short_title = project
html_show_sourcelink = False
html_last_updated_fmt = today_fmt

# Page layout following the Carbon design system on top of the theme,
# see .templates/layout.html, and style adaptions of the Algolia search
# field
html_static_path = ['.static']
html_css_files = ['layout.css', 'algolia.css']
html_js_files = ['layout.js']

# -- Options for LaTeX/PDF output --------------------------------------------

latex_elements = {
    'papersize': 'a4paper',
    'pointsize': '11pt'
}

latex_logo = '../../images/flake-pilot-logo-text.png'

latex_documents = [
    (
        root_doc, 'flake-pilot.tex', 'Flake Pilot User Guide',
        author, 'manual'
    )
]
