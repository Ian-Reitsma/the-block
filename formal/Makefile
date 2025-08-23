VERSION ?= v2025.08.07
FSTAR_HOME ?= $(CURDIR)/.fstar/$(VERSION)
FSTAR ?= $(FSTAR_HOME)/bin/fstar.exe
FSTAR_OPTS = --include . --include $(FSTAR_HOME)/lib

SOURCES = Fee_v2.fst Compute_market.fst Compute_market_invariants.fst
TARGETS = $(SOURCES:.fst=.checked)

all: $(TARGETS)

%.checked: %.fst | $(FSTAR)
	$(FSTAR) $(FSTAR_OPTS) $<
	@touch $@

$(FSTAR):
	FSTAR_VERSION=$(VERSION) FSTAR_HOME=$(FSTAR_HOME) ../scripts/install_fstar.sh >/dev/null

clean:
	rm -f $(TARGETS)
