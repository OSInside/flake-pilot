# Configuration file for the Sphinx documentation builder
#
# Flake Pilot user guide
#
# Build the guide with:
#
#     make -C doc/user_guide html
#
import re
from pathlib import Path

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
    manifest = Path(__file__).resolve().parents[2] / 'common' / 'Cargo.toml'
    try:
        version = re.search(
            r'^version\s*=\s*"([^"]+)"', manifest.read_text(), re.MULTILINE
        )
    except OSError:
        return ''
    return version.group(1) if version else ''


release = flake_pilot_version()
version = '.'.join(release.split('.')[:2])

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

exclude_patterns = ['build', 'Thumbs.db', '.DS_Store']

root_doc = 'index'

# Most examples in this guide are shell sessions. Blocks which
# contain something else name their language explicitly
highlight_language = 'text'

pygments_style = 'sphinx'

today_fmt = '%B %Y'

# -- Options for HTML output -------------------------------------------------

try:
    import sphinx_rtd_theme  # noqa: F401
    html_theme = 'sphinx_rtd_theme'
    html_theme_options = {
        'collapse_navigation': False,
        'navigation_depth': 3,
        'style_external_links': True,
        'prev_next_buttons_location': 'both'
    }
except ImportError:
    # The guide builds with the themes shipped by Sphinx as well
    html_theme = 'alabaster'
    html_theme_options = {}

html_title = f'{project} User Guide'
html_short_title = project
html_static_path = ['_static']
html_css_files = ['custom.css']
html_show_sourcelink = False
html_last_updated_fmt = today_fmt

# -- Options for LaTeX/PDF output --------------------------------------------

latex_elements = {
    'papersize': 'a4paper',
    'pointsize': '11pt'
}

latex_documents = [
    (
        root_doc, 'flake-pilot.tex', 'Flake Pilot User Guide',
        author, 'manual'
    )
]
