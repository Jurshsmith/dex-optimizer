
CARGO ?= cargo
BIN ?= optimizer
ARGS ?=
PROFILE ?=
BENCH ?=

.PHONY: build
build:
	$(CARGO) build $(if $(strip $(PROFILE)),--profile $(PROFILE),)

.PHONY: run
run:
	$(CARGO) run $(if $(strip $(BIN)),--bin $(BIN),) $(if $(strip $(ARGS)), -- $(ARGS),)

.PHONY: run-%
run-%:
	$(CARGO) run --bin $* $(if $(strip $(ARGS)), -- $(ARGS),)

.PHONY: lint
lint:
	$(CARGO) fmt --check
	$(CARGO) clippy -- -D warnings

.PHONY: test tests
test: tests

.PHONY: tests
tests:
	$(CARGO) test

.PHONY: benches
benches: bench-aos bench-soa bench-pipeline

.PHONY: bench
bench:
	$(CARGO) bench $(if $(strip $(BENCH)),--bench $(BENCH),)

.PHONY: bench-layout
bench-layout:  bench-aos bench-soa
 
.PHONY: bench-aos
bench-aos:
	$(CARGO) bench --bench bench_aos

.PHONY: bench-soa
bench-soa:
	$(CARGO) bench --bench bench_soa


.PHONY: bench-pipeline
bench-pipeline:
	$(CARGO) bench --bench pipeline

.PHONY: bench-%
bench-%:
	$(CARGO) bench --bench $*
