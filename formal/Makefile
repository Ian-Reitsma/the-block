FSTAR ?= fstar.exe
FSTAR_OPTS = --include . --include $(FSTAR_HOME)/ulib

all: Fee_v2.checked compute_market.checked

Fee_v2.checked: Fee_v2.fst
	$(FSTAR) $(FSTAR_OPTS) $<
	@touch $@

compute_market.checked: Compute_market.fst
	$(FSTAR) $(FSTAR_OPTS) $<
	@touch $@

clean:
	rm -f *.checked
