# Copr's "make srpm" build method: https://docs.pagure.org/copr.copr/user_documentation.html#make-srpm
# Copr runs `make -f .copr/Makefile srpm outdir=<dir> spec=<spec>` in a checkout of this repo
# (with network access), then builds the source RPM offline.
outdir ?= $(CURDIR)/out
spec ?= packaging/fedora/craftspace.spec
top := $(abspath $(dir $(lastword $(MAKEFILE_LIST)))/..)

srpm:
	command -v cargo >/dev/null || dnf -y install cargo rust gcc
	command -v rpmbuild >/dev/null || dnf -y install rpm-build
	command -v git >/dev/null || dnf -y install git
	$(eval VERSION := $(shell sed -n 's/^version = "\(.*\)"/\1/p' $(top)/Cargo.toml | head -1))
	rm -rf $(top)/.copr/build && mkdir -p $(top)/.copr/build/craftspace-$(VERSION) $(outdir)
	cd $(top) && git ls-files -z | xargs -0 cp --parents -t .copr/build/craftspace-$(VERSION)
	cd $(top)/.copr/build/craftspace-$(VERSION) && mkdir -p .cargo && cargo vendor --locked vendor > .cargo/config.toml
	tar -C $(top)/.copr/build -czf $(outdir)/craftspace-$(VERSION)-vendored.tar.gz craftspace-$(VERSION)
	sed 's/^Version:.*/Version:        $(VERSION)/' $(top)/$(spec) > $(top)/.copr/build/craftspace.spec
	rpmbuild -bs --define "_sourcedir $(outdir)" --define "_srcrpmdir $(outdir)" $(top)/.copr/build/craftspace.spec
	rm -rf $(top)/.copr/build
