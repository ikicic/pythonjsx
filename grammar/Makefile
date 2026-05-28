.PHONY: build clean

.DEFAULT_GOAL := build

BUILD_DIR := ./build
GENERATED_DIR := ./src
TREE_SITTER_PYTHON_DIR := ../node_modules/tree-sitter-python/src
TREE_SITTER_HEADER_FILES := \
	$(GENERATED_DIR)/tree_sitter/alloc.h \
	$(GENERATED_DIR)/tree_sitter/array.h \
	$(GENERATED_DIR)/tree_sitter/parser.h

# Platform detection
UNAME_S := $(shell uname -s)
ifeq ($(UNAME_S),Darwin)
	LIB_EXT := .dylib
	CC := $(if $(CC),$(CC),cc)
endif
ifeq ($(UNAME_S),Linux)
	LIB_EXT := .so
	CC := $(if $(CC),$(CC),gcc)
endif
ifndef LIB_EXT
	LIB_EXT := .dll
	CC := $(if $(CC),$(CC),gcc)
endif

all: build
	true

$(TREE_SITTER_HEADER_FILES): $(GENERATED_DIR)/%: $(TREE_SITTER_PYTHON_DIR)/%
	@mkdir -p $(dir $@)
	cp $< $@

$(GENERATED_DIR)/parser.c: grammar.js
	../node_modules/.bin/tree-sitter generate

$(GENERATED_DIR)/scanner.c: $(TREE_SITTER_PYTHON_DIR)/scanner.c
	@mkdir -p $(dir $@)
	sed 's/tree_sitter_python/tree_sitter_pythonjsx/g' $< > $@

build: $(TREE_SITTER_HEADER_FILES) $(GENERATED_DIR)/parser.c $(GENERATED_DIR)/scanner.c
	@echo "Compiling shared library..."
	@mkdir -p $(BUILD_DIR)
	$(CC) -shared -fPIC -o $(BUILD_DIR)/pythonjsx_grammar$(LIB_EXT) \
		-I $(GENERATED_DIR) \
		-std=c11 \
		$(GENERATED_DIR)/parser.c \
		$(GENERATED_DIR)/scanner.c
	@echo "Grammar built successfully"

clean:
	rm -rf $(BUILD_DIR)
	find . -type d -name __pycache__ -exec rm -r {} +
	find . -type f -name "*.pyc" -delete
	find . -type f -name "*.pyo" -delete
