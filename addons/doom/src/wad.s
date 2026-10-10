# The WAD, compiled into the module. WAD_PATH is set by build.sh.
	.section .rodata.cs_wad,"",@
	.globl cs_wad_data
	.globl cs_wad_end
	.p2align 4
cs_wad_data:
	.incbin WAD_PATH
cs_wad_end:
	.size cs_wad_data, cs_wad_end-cs_wad_data
	.size cs_wad_end, 0
