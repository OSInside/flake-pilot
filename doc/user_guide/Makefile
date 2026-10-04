# Makefile for the Flake Pilot user guide
#
# The build requires python-sphinx and, for the default HTML style,
# the sphinx_rtd_theme. All of it is provided by the docs group of
# the poetry setup in the toplevel directory:
#
#     make -C ../.. setup

SPHINXBUILD ?= sphinx-build
SPHINXOPTS  ?= -W --keep-going
SOURCEDIR   = .
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
