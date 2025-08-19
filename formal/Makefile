FSTAR ?= fstar.exe
FSTAR_OPTS = --include .

all: fee_v2.checked

fee_v2.checked: fee_v2.fst
	$(FSTAR) $(FSTAR_OPTS) $<
	@touch $@

clean:
	rm -f *.checked
