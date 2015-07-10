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

vfs_node_t* fs_root = NULL;		// The root of the filesystem.

ssize_t read_fs(fildes_t* file, uint8_t* buffer, size_t size)
{
	vfs_node_t* node = file->node;
	ssize_t ret = -EINVAL;

	if (BUILTIN_EXPECT(!node || !buffer, 0))
		return ret;

	spinlock_lock(&node->lock);
	// Has the node got a read callback?
	if (node->read != 0)
		ret = node->read(file, buffer, size);
	spinlock_unlock(&node->lock);

	return ret;
}

ssize_t write_fs(fildes_t* file, uint8_t* buffer, size_t size)
{
	vfs_node_t* node = file->node;
	ssize_t ret = -EINVAL;

	if (BUILTIN_EXPECT(!node || !buffer, 0))
		return ret;

	spinlock_lock(&node->lock);
	// Has the node got a write callback?
	if (node->write != 0)
		ret = node->write(file, buffer, size);
	spinlock_unlock(&node->lock);

	return ret;
}

int open_fs(fildes_t* file, const char* name)
{
	uint32_t ret = 0, i, j = 1;
	vfs_node_t* file_node = NULL;  /* file node */
	vfs_node_t* dir_node = NULL;
	char fname[MAX_FNAME];

	if (BUILTIN_EXPECT(!name, 0))
		return ret;

	if (name[0] == '/')
		file_node = fs_root;

	while((name[j] != '\0') || ((file_node != NULL) && (file_node->type == FS_DIRECTORY))) {
		i = 0;
		while((name[j] != '/') && (name[j] != '\0')) {
			fname[i] = name[j];
			i++; j++;
		}
		fname[i] = '\0';
		dir_node = file_node; /* file must be a directory */
		file_node = finddir_fs(dir_node, fname);
		if (name[j] == '/') 
			j++;
	}

	//kprintf("dir_node = %p, file_node = %p, name = %s \n", dir_node, file_node, fname);

	/* file exists */
	if(file_node) {
		spinlock_lock(&file_node->lock);
		file->node = file_node;
		// Has the file_node got an open callback?
		if (file_node->open != 0)
			ret = file->node->open(file, NULL);
		spinlock_unlock(&file_node->lock);
	} else if (dir_node) { /* file doesn't exist or opendir was called */
		spinlock_lock(&dir_node->lock);
		file->node = dir_node;
		// Has the dir_node got an open callback?
		if (dir_node->open != 0)
			ret = dir_node->open(file, fname);
		spinlock_unlock(&dir_node->lock);
	} else {
		ret = -ENOENT;
	}

	return ret;
}

int close_fs(fildes_t* file)
{
	int ret = -EINVAL;

	if (BUILTIN_EXPECT(!(file->node), 0))
		return ret;

	spinlock_lock(&file->node->lock);
	// Has the node got a close callback?
	if (file->node->close != 0)
		ret = file->node->close(file);
	spinlock_unlock(&file->node->lock);

	return ret;
}

struct dirent* readdir_fs(vfs_node_t * node, uint32_t index)
{
	struct dirent* ret = NULL;

	if (BUILTIN_EXPECT(!node, 0))
		return ret;

	spinlock_lock(&node->lock);
	// Is the node a directory, and does it have a callback?
	if ((node->type == FS_DIRECTORY) && node->readdir != 0)
		ret = node->readdir(node, index);
	spinlock_unlock(&node->lock);

	return ret;
}

vfs_node_t* finddir_fs(vfs_node_t* node, const char *name)
{
	vfs_node_t* ret = NULL;

	if (BUILTIN_EXPECT(!node, 0))
		return ret;

	spinlock_lock(&node->lock);
	// Is the node a directory, and does it have a callback?
	if ((node->type == FS_DIRECTORY) && node->finddir != 0)
		ret = node->finddir(node, name);
	spinlock_unlock(&node->lock);

	return ret;
}

vfs_node_t* mkdir_fs(vfs_node_t* node, const char *name)
{
	vfs_node_t* ret = NULL;

	if (BUILTIN_EXPECT(!node, 0))
		return ret;

	spinlock_lock(&node->lock);
	if (node->mkdir != 0)
		ret = node->mkdir(node, name);
	spinlock_unlock(&node->lock);

	return ret;
}

vfs_node_t* findnode_fs(const char* name)
{
	uint32_t i, j = 1;
	vfs_node_t* ret = NULL;
	char fname[MAX_FNAME];

	if (BUILTIN_EXPECT(!name, 0))
		return ret;

	if (name[0] == '/')
		ret = fs_root;

	while((name[j] != '\0') && ret) {
		i = 0;
		while((name[j] != '/') && (name[j] != '\0')) {
			fname[i] = name[j];
			i++; j++;
		}
		fname[i] = '\0';
		ret = finddir_fs(ret, fname);
		if (name[j] == '/') 
			j++;
	}

	return ret;
}

void list_fs(vfs_node_t* node, uint32_t depth)
{
	int j, i = 0;
	dirent_t* dirent = NULL;
	fildes_t* file = kmalloc(sizeof(fildes_t));
	file->offset = 0;
	file->flags = 0;


	while ((dirent = readdir_fs(node, i)) != 0) {
		for(j=0; j<depth; j++)
			kputs("  ");
		kprintf("%s\n", dirent->name);

		if (strcmp(dirent->name, ".") && strcmp(dirent->name, "..")) {
			vfs_node_t *new_node = finddir_fs(node, dirent->name);
			if (new_node) {
				if (new_node->type == FS_FILE) {
					char buff[16] = {[0 ... 15] = 0x00};

					file->node = new_node;
					file->offset = 0;
					file->flags = 0;

					read_fs(file, (uint8_t*)buff, 8);
					for(j=0; j<depth+1; j++)
						kputs("  ");
					kprintf("content: %s\n", buff);
				} else list_fs(new_node, depth+1);
			}
		}

		i++;
	}
	kfree(file);
}
