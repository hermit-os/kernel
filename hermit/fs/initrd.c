/*
 * Copyright (c) 2010, Stefan Lankes, RWTH Aachen University
 * All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions are met:
 *    * Redistributions of source code must retain the above copyright
 *      notice, this list of conditions and the following disclaimer.
 *    * Redistributions in binary form must reproduce the above copyright
 *      notice, this list of conditions and the following disclaimer in the
 *      documentation and/or other materials provided with the distribution.
 *    * Neither the name of the University nor the names of its contributors
 *      may be used to endorse or promote products derived from this
 *      software without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED
 * WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
 * DISCLAIMED. IN NO EVENT SHALL THE REGENTS OR CONTRIBUTORS BE LIABLE FOR ANY
 * DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES
 * (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES;
 * LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND
 * ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
 * (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
 * SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */

#include <hermit/stdlib.h>
#include <hermit/stdio.h>
#include <hermit/string.h>
#include <hermit/fs.h>
#include <hermit/errno.h>
#include <hermit/spinlock.h>
#include <asm/multiboot.h>
#include <asm/processor.h>

static vfs_node_t initrd_root;

/*
 * Note that linker symbols are not variables, they have no memory allocated for
 * maintaining a value, rather their address is their value.
 */
extern const void initrd_start, initrd_end;

#define INITRD_MAGIC_NUMBER     0x4711

/* 
 * Grub maps the initrd as module into the address space of the kernel.
 * The module begins with the header "initrd_header_t", which describes
 * the number of mounted files and the mount point. The header follows
 * by the struct "initrd_file_desc_t" for each file, which will be mounted.
 * This struct describes the file properties and the position in the 
 * module.
 */
typedef struct {
	uint32_t magic;
	uint32_t nfiles;
	char mount_point[MAX_FNAME];
} initrd_header_t;

typedef struct {
	uint32_t length;
	uint32_t offset;
	char fname[MAX_FNAME];
} initrd_file_desc_t;

static ssize_t initrd_read(fildes_t* file, uint8_t* buffer, size_t size)
{
	vfs_node_t* node = file->node;

	uint32_t i, pos = 0, found = 0;
	off_t offset = 0;
	char* data = NULL;
	block_list_t* blist = &node->block_list;

	if (file->flags & O_WRONLY)
		return -EACCES;

	/* init the tmp offset */
	offset = file->offset;

	/* searching for the valid data block */
	if (offset) {
		pos = offset / node->block_size;
		offset = offset % node->block_size;
	}
	do {
		for(i=0; i<MAX_DATABLOCKS && !data; i++) {
			if (blist->data[i]) {
				found++;
				if (found > pos)
					data = (char*) blist->data[i];
			}
		}
			
		blist = blist->next;
	} while(blist && !data);
	
	if (BUILTIN_EXPECT(!data, 0))
		return 0;

	/* 
	 * If the data block is not large engough, 
	 * we copy only the rest of the current block.
	 * The user has to restart the read operation
	 * for the next block.
	 */
	if ((offset + size) >= node->block_size)
		size = node->block_size - offset;

	memcpy(buffer, data + offset, size);

	file->offset += size;
	return size;
}

static ssize_t initrd_emu_readdir(fildes_t* file, uint8_t* buffer, size_t size)
{
	vfs_node_t* node = file->node;

	uint32_t i, j, k, count;
	uint32_t index = file->offset;
	dirent_t* dirent;
	dir_block_t* dirblock;
	block_list_t* blist = &node->block_list;

	do {
		for(i=0,count=0; i<MAX_DATABLOCKS; i++) {
			dirblock = (dir_block_t*) blist->data[i];
			for(j=0; dirblock && j<MAX_DIRENTRIES; j++) {
				dirent = &dirblock->entries[j];
				if (dirent->vfs_node) {
					count++;
					if (count > index) {
						k=0;
						do { 
							buffer[k] = dirent->name[k];
							k++;
						} while(dirent->name[k] != '\0');
						file->offset++;
						return k;
					}
				}
			}
		}
	
		blist = blist->next;
	} while(blist);

	return -EINVAL;
}

static ssize_t initrd_write(fildes_t* file, uint8_t* buffer, size_t size)
{
	uint32_t i, pos = 0, found = 0;
	off_t offset = 0;
	char* data = NULL;
	vfs_node_t* node = file->node;
	block_list_t* blist = &node->block_list;

	if (file->flags & O_RDONLY) 
		return -EACCES;
				
	if (file->flags & O_APPEND)
		file->offset = node->block_size;

	/* init the tmp offset */
	offset = file->offset;

	/* searching for the valid data block */
	if (offset) {
		pos = offset / MAX_DATAENTRIES;
		offset = offset % MAX_DATAENTRIES;
	}

	do {
		for (i = 0; i < MAX_DATABLOCKS && !data; i++) {
			if ((size + offset) >= MAX_DATAENTRIES)
				size = MAX_DATAENTRIES - offset;
			if(!blist->data[i]) { 
				blist->data[i] = (data_block_t*) 
					kmalloc(sizeof(data_block_t));
				if (blist->data[i])
					memset(blist->data[i], 0x00, 
						sizeof(data_block_t));
			}
			found++;
			if (found > pos) {
				data = (char*) blist->data[i];
			}
		} 

		if (!blist->next) {
			blist->next = (block_list_t*) 
				kmalloc(sizeof(block_list_t));
			if (blist->next) 
				memset(blist->next, 0x00, 
					sizeof(block_list_t));
		}
		blist = blist->next;
	} while(blist && !data);

	/* you may have to increase nodesize */
	if (node->block_size < (file->offset + size))
		node->block_size = file->offset + size;
	/* 
	 * If the data block is not large engough, 
	 * we copy only the rest of the current block.
	 * The user has to restart the write operation
	 * for the next block.
         */
	memcpy(data + offset, buffer, size);
	file->offset += size;
	return size;
}

static int initrd_open(fildes_t* file, const char* name)
{
	if (file->node->type == FS_FILE) {
		if ((file->flags & O_CREAT) && (file->flags & O_EXCL)) 
			return -EEXIST;
		
		/* in the case of O_TRUNC kfree all the nodes */
		if (file->flags & O_TRUNC) {
			uint32_t i;
			char* data = NULL;
			block_list_t* blist = &file->node->block_list;
			block_list_t* lastblist = NULL;

			/* the first blist pointer have do remain valid. */
			for(i=0; i<MAX_DATABLOCKS && !data; i++) {
				if (blist->data[i]) {
					kfree(blist->data[i]);
				}
			}
			if (blist->next) {
				lastblist = blist;
				blist = blist->next;
				lastblist->next = NULL;

				/* kfree all other blist pointers */
				do {
					for(i=0; i<MAX_DATABLOCKS && !data; i++) {
						if (blist->data[i]) {
							kfree(blist->data[i]);
						}
					}
					lastblist = blist;
					blist = blist->next;
					kfree(lastblist);
				} while(blist);
			}

			/* reset the block_size */
			file->node->block_size = 0;
		}
	}

	if (file->node->type == FS_DIRECTORY) {

		/* opendir was called: */
		if (name[0] == '\0') 
			return 0;
	
		/* open file was called: */
		if (!(file->flags & O_CREAT)) 
			return -ENOENT;

		uint32_t i, j;
		block_list_t* blist = NULL;
		/* CREATE FILE */
		vfs_node_t* new_node = kmalloc(sizeof(vfs_node_t));
		if (BUILTIN_EXPECT(!new_node, 0))
			return -EINVAL;
		
		blist = &file->node->block_list;
		dir_block_t* dir_block;
		dirent_t* dirent;
		
		memset(new_node, 0x00, sizeof(vfs_node_t));
		new_node->type = FS_FILE;
		new_node->read = initrd_read;
		new_node->write = initrd_write;
		new_node->open = initrd_open;
		spinlock_init(&new_node->lock);

		/* create a entry for the new node in the directory block of current node */
		do {
			for(i=0; i<MAX_DATABLOCKS; i++) {
				if (blist->data[i]) {
					dir_block = (dir_block_t*) blist->data[i];
					for(j=0; j<MAX_DIRENTRIES; j++) {
						dirent = &dir_block->entries[j];
						if (!dirent->vfs_node) {
							dirent->vfs_node = new_node;
							strncpy(dirent->name, (char*) name, MAX_FNAME);
							goto exit_create_file; // there might be a better Solution ***************
						}
 					}
				}
 			}
			 /* if all blocks are reserved, we have  to allocate a new one */
			if (!blist->next) {
				blist->next = (block_list_t*) kmalloc(sizeof(block_list_t));
				if (blist->next)
					memset(blist->next, 0x00, sizeof(block_list_t));
			}

			blist = blist->next;
		} while(blist);

exit_create_file:
		file->node = new_node;
		file->node->block_size = 0;

	}
	return 0;
}


static dirent_t* initrd_readdir(vfs_node_t* node, uint32_t index)
{
	uint32_t i, j, count;
	dirent_t* dirent;
	dir_block_t* dirblock;
	block_list_t* blist = &node->block_list;

	do {
		for(i=0,count=0; i<MAX_DATABLOCKS; i++) {
			dirblock = (dir_block_t*) blist->data[i];
			for(j=0; dirblock && j<MAX_DIRENTRIES; j++) {
				dirent = &dirblock->entries[j];
				if (dirent->vfs_node) {
					count++;
					if (count > index)
						return dirent;
				}
			}
		}

		blist = blist->next;
	} while(blist);

	return NULL;
}

static vfs_node_t* initrd_finddir(vfs_node_t* node, const char *name)
{
	uint32_t i, j;
	dir_block_t* dirblock;
	dirent_t* dirent;
	block_list_t* blist = &node->block_list;

	do {
		for(i=0; i<MAX_DATABLOCKS; i++) {
			dirblock = (dir_block_t*) blist->data[i];
			for(j=0; dirblock && j<MAX_DIRENTRIES; j++) {	
				dirent = &dirblock->entries[j];
				if (!strncmp(dirent->name, name, MAX_FNAME))
					return dirent->vfs_node;
			}	
		}

		blist = blist->next;
	} while(blist);

	return NULL;
}

static vfs_node_t* initrd_mkdir(vfs_node_t* node, const char* name)
{
	uint32_t i, j;
	dir_block_t* dir_block;
	dir_block_t* tmp;
	dirent_t* dirent;
	vfs_node_t* new_node;
	block_list_t* blist = &node->block_list;

	if (BUILTIN_EXPECT(node->type != FS_DIRECTORY, 0))
		return NULL;

	/* exists already a entry with same name? */
	if (initrd_finddir(node, name))
		return NULL;

	new_node = kmalloc(sizeof(vfs_node_t));
	if (BUILTIN_EXPECT(!new_node, 0))
		return NULL;

	memset(new_node, 0x00, sizeof(vfs_node_t));
	new_node->type = FS_DIRECTORY;
	new_node->read = &initrd_emu_readdir;
	new_node->readdir = &initrd_readdir;
	new_node->finddir = &initrd_finddir;
	new_node->mkdir = &initrd_mkdir;
	new_node->open = &initrd_open;
	spinlock_init(&new_node->lock);

	/* create default directory entry */
	dir_block = (dir_block_t*) kmalloc(sizeof(dir_block_t));
	if (BUILTIN_EXPECT(!dir_block, 0))
		goto out;
	memset(dir_block, 0x00, sizeof(dir_block_t));
	new_node->block_list.data[0] = dir_block;
	strncpy(dir_block->entries[0].name, ".", MAX_FNAME);
	dir_block->entries[0].vfs_node = new_node;
	strncpy(dir_block->entries[1].name, "..", MAX_FNAME);
	dir_block->entries[1].vfs_node = node;

	do {
		/* searching for a free directory block */
		for(i=0; i<MAX_DATABLOCKS; i++) {
			if (blist->data[i]) {
				tmp = (dir_block_t*) blist->data[i];
				for(j=0; j<MAX_DIRENTRIES; j++) {
					dirent = &tmp->entries[j];
					if (!dirent->vfs_node) {
						dirent->vfs_node = new_node;
						strncpy(dirent->name, name, MAX_FNAME);
						return new_node;
					}
				}
			}
		}

		/* if all blocks are reserved, we have  to allocate a new one */
		if (!blist->next) {
			blist->next = (block_list_t*) kmalloc(sizeof(block_list_t));
			if (blist->next)
				memset(blist->next, 0x00, sizeof(block_list_t));
		}

		blist = blist->next;
	} while(blist);

	kfree(dir_block);
out:
	kfree(new_node);

	return NULL;
}

int initrd_init(void)
{
	dir_block_t* dir_block;
	vfs_node_t* tmp;
	uint32_t i, j, k, l;
#if 0
	uint32_t mods_count = 0;
	multiboot_module_t* mmodule = NULL;

	if (mb_info && (mb_info->flags & MULTIBOOT_INFO_MODS)) {
		mmodule = (multiboot_module_t*) ((size_t) mb_info->mods_addr);
		mods_count = mb_info->mods_count;
	}
#endif

	/* Initialize the root directory. */
	fs_root = &initrd_root;
	memset(&initrd_root, 0x00, sizeof(vfs_node_t));
	initrd_root.type = FS_DIRECTORY;
	initrd_root.read = &initrd_emu_readdir;
	initrd_root.readdir = &initrd_readdir;
	initrd_root.finddir = &initrd_finddir;
	initrd_root.mkdir = &initrd_mkdir;
	initrd_root.open = &initrd_open;
	spinlock_init(&initrd_root.lock);

	/* create default directory block */
	dir_block = (dir_block_t*) kmalloc(sizeof(dir_block_t));
	if (BUILTIN_EXPECT(!dir_block, 0))
		return -ENOMEM;
	memset(dir_block, 0x00, sizeof(dir_block_t));
	initrd_root.block_list.data[0] = dir_block;
	strncpy(dir_block->entries[0].name, ".", MAX_FNAME);
	dir_block->entries[0].vfs_node = fs_root;
	strncpy(dir_block->entries[1].name, "..", MAX_FNAME);
	dir_block->entries[1].vfs_node = fs_root;

	/* create the directory bin and dev */
	mkdir_fs(fs_root, "bin");
	//mkdir_fs(fs_root, "sbin");
	mkdir_fs(fs_root, "dev");
	//mkdir_fs(fs_root, "tmp");

	/* 
	 * Currently we attached the initrd at the end of the kernel
	 * => we don't longer use grub to load initrd as module
	 */
#if 0
	/* For every module.. */
	for(i=0; i<mods_count; i++) {
		initrd_header_t* header = (initrd_header_t*) ((size_t) mmodule[i].mod_start);
#else
	kprintf("Initrd starts at %p and ends at %p\n",  &initrd_start, &initrd_end);

	for(i=0; i<1; i++) {
		initrd_header_t* header = (initrd_header_t*) &initrd_start;
#endif

		initrd_file_desc_t* file_desc;
		vfs_node_t* new_node;

		if (BUILTIN_EXPECT(header->magic != INITRD_MAGIC_NUMBER, 0)) {
			kprintf("Invalid magic number for a init ram disk: 0x%x\n", header->magic);
			continue;
		}

		tmp = findnode_fs(header->mount_point);
		if (BUILTIN_EXPECT(!tmp, 0)) {
			kprintf("Did not found mount point %s.\n", header->mount_point);
			continue;
		}

		if (BUILTIN_EXPECT(tmp->type != FS_DIRECTORY, 0)) {
			kprintf("%s is not a valid mount point.\n", header->mount_point);
			continue;
		}

		file_desc = (initrd_file_desc_t*) (header + 1);
		for(j=0; j<header->nfiles; j++) {
			block_list_t* blist;
			dir_block_t* dir_block;
			dirent_t* dirent;

			if (finddir_fs(tmp, file_desc->fname)) {
				kprintf("Error: %s alreay exits\n", file_desc->fname);
				goto next_file;
			}

			/* create a new node and map the module as data block */
			new_node = kmalloc(sizeof(vfs_node_t));
			if (BUILTIN_EXPECT(!new_node, 0)) {
				kprintf("Not enough memory to create new initrd node\n");
				goto next_file;
			}
			memset(new_node, 0x00, sizeof(vfs_node_t));
			new_node->type = FS_FILE;
			new_node->read = initrd_read;
			new_node->write = initrd_write;
			new_node->open = initrd_open;
			new_node->block_size = file_desc->length;
			new_node->block_list.data[0] = ((char*) header) + file_desc->offset;
			spinlock_init(&new_node->lock);

			/* create a entry for the new node in the directory block of current node */
			blist = &tmp->block_list;
			do {
				for(k=0; k<MAX_DATABLOCKS; k++) {
					if (blist->data[k]) {
						dir_block = (dir_block_t*) blist->data[k];
 						for(l=0; l<MAX_DIRENTRIES; l++) {
 							dirent = &dir_block->entries[l];
							if (!dirent->vfs_node) {
 								dirent->vfs_node = new_node;
								strncpy(dirent->name, file_desc->fname, MAX_FNAME);
								goto next_file;
							}
 						}
					}
 				}

				 /* if all blocks are reserved, we have  to allocate a new one */
	 			if (!blist->next) {
					blist->next = (block_list_t*) kmalloc(sizeof(block_list_t));
					if (blist->next)
						memset(blist->next, 0x00, sizeof(block_list_t));
				}

 				blist = blist->next;
			} while(blist);
next_file:
			file_desc++;
		}
	}

	return 0;
}
