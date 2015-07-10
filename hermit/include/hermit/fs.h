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

/**
 * @author Stefan Lankes
 * @file include/hermit/fs.h
 * @brief Filesystem related functions and structures
 */

#ifndef __FS_H__
#define __FS_H__

#include <hermit/stddef.h>
#include <hermit/spinlock_types.h>

#define FS_FILE		0x01
#define FS_DIRECTORY	0x02
#define FS_CHARDEVICE	0x03
//#define FS_BLOCKDEVICE 0x04
//#define FS_PIPE        0x05
//#define FS_SYMLINK     0x06
//#define FS_MOUNTPOINT  0x08	// Is the file an active mountpoint?


/*file descriptor init*/
#define NR_OPEN 100

#define _FOPEN          (-1)	/* from sys/file.h, kernel use only */
#define _FREAD		0x0001	/* read enabled */
#define _FWRITE		0x0002	/* write enabled */
#define _FAPPEND	0x0008	/* append (writes guaranteed at the end) */
#define _FMARK		0x0010	/* internal; mark during gc() */
#define _FDEFER		0x0020	/* internal; defer for next gc pass */
#define _FASYNC		0x0040	/* signal pgrp when data ready */
#define _FSHLOCK	0x0080	/* BSD flock() shared lock present */
#define _FEXLOCK	0x0100	/* BSD flock() exclusive lock present */
#define _FCREAT		0x0200	/* open with file create */
#define _FTRUNC		0x0400	/* open with truncation */
#define _FEXCL		0x0800	/* error on open if file exists */
#define _FNBIO		0x1000	/* non blocking I/O (sys5 style) */
#define _FSYNC		0x2000	/* do all writes synchronously */
#define _FNONBLOCK	0x4000	/* non blocking I/O (POSIX style) */
#define _FNDELAY	_FNONBLOCK	/* non blocking I/O (4.2 style) */
#define _FNOCTTY	0x8000	/* don't assign a ctty on this open */

/*open flags*/
#define O_RDONLY	0
#define O_WRONLY	1
#define O_RDWR		2
#define O_APPEND	_FAPPEND
#define O_CREAT		_FCREAT
#define O_TRUNC		_FTRUNC
#define O_EXCL		_FEXCL
#define O_SYNC		_FSYNC
#define O_NONBLOCK	_FNONBLOCK
#define O_NOCTTY	_FNOCTTY

/*lseek defines*/
#ifndef SEEK_SET
#define	SEEK_SET	0	/* set file offset to offset */
#endif
#ifndef SEEK_CUR
#define	SEEK_CUR	1	/* set file offset to current plus offset */
#endif
#ifndef SEEK_END
#define	SEEK_END	2	/* set file offset to EOF plus offset */
#endif

struct vfs_node;
struct fildes;
/** @defgroup fsprototypes FS access function prototypes
 *
 * These typedefs define the type of callbacks - called when read/write/open/close are called.\n
 * They aren't as well documented as the read_fs and so on functions. Just look there for further information.
 *
 * @{
 */

/** @brief Read function pointer */
typedef ssize_t (*read_type_t) (struct fildes *, uint8_t*, size_t);
/** @brief Write function pointer */
typedef ssize_t (*write_type_t) (struct fildes *, uint8_t*, size_t);
/** @brief Open function pointer */
typedef int (*open_type_t) (struct fildes *, const char *name);
/** @brief Close function pointer */
typedef int (*close_type_t) (struct fildes *);
/** @brief Read directory function pointer */
typedef struct dirent *(*readdir_type_t) (struct vfs_node *, uint32_t);
/** @brief Find directory function pointer */
typedef struct vfs_node *(*finddir_type_t) (struct vfs_node *, const char *name);
/** @brief Make directory function pointer */
typedef struct vfs_node *(*mkdir_type_t) (struct vfs_node *, const char *name);

/** @} */

#define MAX_DATABLOCKS		12
#define MAX_DIRENTRIES		32
#define MAX_DATAENTRIES 	4096


/** @brief Block list structure. VFS nodes keep those in a list */
typedef struct block_list {
	/// Array with pointers to data blocks
	void* data[MAX_DATABLOCKS];
	/// Pointer to the next block_list in the list
	struct block_list* next;
} block_list_t;

typedef struct vfs_node {
	/// The permissions mask.
	uint32_t mask;		
	/// The owning user.
	uint32_t uid;		
	/// The owning group.
	uint32_t gid;		
	/// Includes the node type. See the defines above.
	uint32_t type;		
	/// Open handler function pointer
	open_type_t open;
	/// Close handler function pointer
	close_type_t close;
	/// Read handler function pointer
	read_type_t read;
	/// Write handler function pointer
	write_type_t write;
	/// Read dir handler function pointer
	readdir_type_t readdir;
	/// Find dir handler function pointer
	finddir_type_t finddir;
	/// Make dir handler function pointer
	mkdir_type_t mkdir;
	/// Lock variable to thread-protect this structure
	spinlock_t lock;
	/// Block size
	size_t block_size;
	/// List of blocks
	block_list_t block_list;
} vfs_node_t;

/** @brief file descriptor structure */
typedef struct fildes {
        vfs_node_t* 	node;		/*  */
        off_t 		offset;		/*  */
	int 		flags;		/*  */
	int 		mode;		/*  */
	int 		count;		/* number of tasks using this fd */
} fildes_t, *filp_t;

/** @brief Directory entry structure */
typedef struct dirent {
	/// Directory name
	char name[MAX_FNAME];
	/// Corresponding VFS node pointer
	vfs_node_t* vfs_node;
} dirent_t;

/** @brief Dir block structure which will keep directory entries */
typedef struct {
	/// Array of directory entries
	dirent_t entries[MAX_DIRENTRIES];
} dir_block_t;

typedef struct {
	///Array of data entries
	char entries[MAX_DATAENTRIES];
} data_block_t;


/* Time Value Specification Structures, P1003.1b-1993, p. 261 */
struct timespec {
  long	  tv_sec;   /* Seconds */
  long    tv_nsec;  /* Nanoseconds */
};

struct stat 
{
	uint16_t		st_dev;		/* ID of device containing file */
	uint16_t		st_ino;		/* inode number */
	uint32_t		st_mode;	/* protection */
	unsigned short	st_nlink;	/* number of hard links */
	uint16_t		st_uid;		/* user ID of owner */
	uint16_t		st_gid;		/* group ID of owner */
	uint16_t		st_rdev;	/* device ID (if special file) */
	off_t		st_size;	/* total size, in bytes */
	struct timespec st_atim;	/* time of last access */
	struct timespec st_mtim;	/* time of last modification */
	struct timespec st_ctim;	/* time of last status change */
	uint32_t	st_blksize;	/* blocksize for filesystem I/O */
	uint32_t	st_blocks;	/* number of blocks allocated */
} stat_t;

extern vfs_node_t* fs_root;	// The root of the filesystem.

/** @defgroup fsfunc FS related functions
 *
 * Standard read/write/open/close/mkdir functions. Note that these are all suffixed with
 * _fs to distinguish them from the read/write/open/close which deal with file descriptors, 
 * not file nodes.
 *
 * @{
 */

/** @brief Read from file system into the buffer
 * @param file Pointer to the file descriptor to read from
 * @param buffer Pointer to buffer to write into
 * @param size Number of bytes to read
 * @return
 * - number of bytes copied (size)
 * - 0 on error
 */
ssize_t read_fs(fildes_t* file, uint8_t* buffer, size_t size);

/** @brief Write into the file system from the buffer 
 * @param file Pointer to the file descriptor to write to
 * @param buffer Pointer to buffer to read from
 * @param size Number of bytes to read
 * @return
 * - number of bytes copied (size)
 * - 0 on error
 */
ssize_t write_fs(fildes_t* file, uint8_t* buffer, size_t size);

/** @brief Yet to be documented */
int open_fs(fildes_t* file, const char* fname);

/** @brief Yet to be documented */
int close_fs(fildes_t * file);

/** @brief Get dir entry at index
 * @param node VFS node to get dir entry from
 * @param index Index position of desired dir entry
 * @return
 * - The desired dir entry
 * - NULL on failure
 */
struct dirent *readdir_fs(vfs_node_t * node, uint32_t index);

/** @brief Find a directory by looking for the dir name
 * @param node The node where to start the search from
 * @param name The dir name string
 * @return
 * - a VFS node pointer
 * - NULL on failure
 */
vfs_node_t* finddir_fs(vfs_node_t * node, const char *name);

/** @brief Make a new directory in a VFS node 
 * @param node Pointer to the node where the dir is to create in
 * @param name Name of the new directory
 * @return
 * - new VFS node pointer
 * - NULL on failure
 */
vfs_node_t* mkdir_fs(vfs_node_t* node, const char* name);

/** @brief Find a node within root file system
 * @param name The node name
 * @return
 * - VFS node pointer
 * - NULL on failure
 */
vfs_node_t* findnode_fs(const char* name);

/** @brief List a filesystem hirachically */
void list_fs(vfs_node_t* node, uint32_t depth);

int initrd_init(void);

#endif
