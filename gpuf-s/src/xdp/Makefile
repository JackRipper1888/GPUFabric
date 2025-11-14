# compile
CLANG   ?= clang
CFLAGS  = -O2 -g -target bpf -D__TARGET_ARCH_x86
INCLUDES = -I/usr/include/$(shell uname -m)-linux-gnu/ -I/usr/include/linux/

# source file and object file
SRC     = xdp_auth_filter.c
OBJ     = xdp_auth_filter.o

# default target
all: $(OBJ)

# compile eBPF program
$(OBJ): $(SRC)
	$(CLANG) $(CFLAGS) $(INCLUDES) -c $< -o $@

# install dependencies (need sudo)
deps:
	sudo apt update
	sudo apt install -y linux-headers-$(shell uname -r)
	sudo apt-get install -y libbpf-dev clang llvm

# clean
clean:
	rm -f $(OBJ)

.PHONY: all clean deps
