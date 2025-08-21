FSTAR_HOME ?= $(shell ../scripts/install_fstar.sh)
FSTAR ?= $(FSTAR_HOME)/bin/fstar.exe
FSTAR_OPTS = --include . --include $(FSTAR_HOME)/lib

SOURCES = Fee_v2.fst Compute_market.fst Compute_market_invariants.fst
TARGETS = $(SOURCES:.fst=.checked)

all: $(TARGETS)

%.checked: %.fst
	$(FSTAR) $(FSTAR_OPTS) $<
	@touch $@

clean:
	rm -f $(TARGETS)
