C_source-$(MODULE) := $(addprefix $(subst _,/,$(MODULE))/,$(filter %.c,$(C_source)))
ASM_source-$(MODULE) := $(addprefix $(subst _,/,$(MODULE))/,$(filter %.asm,$(ASM_source)))
C_source :=
ASM_source :=

OBJS-$(MODULE) := $(C_source-$(MODULE):.c=.o)
OBJS-$(MODULE) += $(ASM_source-$(MODULE):.asm=.o)

$(MODULE): $(OBJS-$(MODULE))

$(NAME).elf: $(OBJS-$(MODULE))

clean: clean-$(MODULE)
clean-$(MODULE): clean-% :
	@echo Cleaning $(subst _,/,$*)
	$Q$(RM) $(OBJS-$*) $(C_source-$*:.c=.dep)

.PHONY: clean-$(MODULE) $(MODULE)

-include $(C_source-$(MODULE):.c=.dep)
