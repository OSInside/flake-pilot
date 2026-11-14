# Makefile for the Flake Pilot user guide
#
# The build requires python-sphinx and the sphinx_rtd_theme, the page
# layout is built on top of it. Both are provided by the docs group
# of the poetry setup in the toplevel directory:
#
#     make -C ../.. setup

SPHINXBUILD ?= sphinx-build
SPHINXOPTS  ?= -W --keep-going
SOURCEDIR   = source
BUILDDIR    = build

.PHONY: help html singlehtml latexpdf linkcheck spelling logo clean

.DEFAULT_GOAL := html

help:
	@$(SPHINXBUILD) -M help "$(SOURCEDIR)" "$(BUILDDIR)" $(SPHINXOPTS)

html:
	@$(SPHINXBUILD) -M html "$(SOURCEDIR)" "$(BUILDDIR)" $(SPHINXOPTS)
	@echo "Find the guide at $(BUILDDIR)/html/index.html"

singlehtml:
	@$(SPHINXBUILD) -M singlehtml "$(SOURCEDIR)" "$(BUILDDIR)" $(SPHINXOPTS)

latexpdf:
	@$(SPHINXBUILD) -M latexpdf "$(SOURCEDIR)" "$(BUILDDIR)" $(SPHINXOPTS)

linkcheck:
	@$(SPHINXBUILD) -M linkcheck "$(SOURCEDIR)" "$(BUILDDIR)" $(SPHINXOPTS)

spelling:
	@$(SPHINXBUILD) -M spelling "$(SOURCEDIR)" "$(BUILDDIR)" $(SPHINXOPTS)

# Regenerate the logo artwork from its sources. Requires inkscape
# and the Adwaita Sans font, the results are part of the repository
IMAGES = ../images

logo:
	inkscape $(IMAGES)/flake-pilot-logo-text.src.svg \
		--export-text-to-path --export-plain-svg \
		--export-filename=$(IMAGES)/flake-pilot-logo-text.svg
	inkscape $(IMAGES)/flake-pilot-logo-text.svg \
		--export-type=png --export-width=1200 \
		--export-filename=$(IMAGES)/flake-pilot-logo-text.png
	inkscape $(IMAGES)/flake-pilot-logo.svg \
		--export-type=png --export-width=512 \
		--export-filename=$(IMAGES)/flake-pilot-logo.png

clean:
	rm -rf $(BUILDDIR)
