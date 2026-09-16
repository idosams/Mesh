#define _GNU_SOURCE

#include <errno.h>
#include <dirent.h>
#include <fcntl.h>
#include <linux/fuse.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mount.h>
#include <sys/stat.h>
#include <sys/statvfs.h>
#include <sys/types.h>
#include <time.h>
#include <unistd.h>

enum {
    ROOT_INO = 1,
    FILE_INO = 2,
    BUFFER_BYTES = 1024 * 1024,
    MAX_CONTENT_BYTES = 16 * 1024 * 1024,
    MAX_ENTRY_NAME_BYTES = 255,
};
static const char *entry_name;
static unsigned char *content;
static size_t content_len;
static size_t content_cap;
static const char *capture_path;
static bool read_only;

static void die(const char *what) {
    perror(what);
    exit(2);
}

static void reserve(size_t wanted) {
    if (wanted > MAX_CONTENT_BYTES) {
        errno = EFBIG;
        die("content too large");
    }
    if (wanted <= content_cap) return;
    size_t next = content_cap ? content_cap : 4096;
    while (next < wanted) next *= 2;
    void *grown = realloc(content, next);
    if (!grown) die("realloc");
    content = grown;
    content_cap = next;
}

static void load_file(const char *path) {
    FILE *file = fopen(path, "rb");
    if (!file) die("open source");
    if (fseek(file, 0, SEEK_END) != 0) die("seek source");
    long length = ftell(file);
    if (length < 0) die("measure source");
    if ((unsigned long)length > MAX_CONTENT_BYTES) {
        errno = EFBIG;
        die("source too large");
    }
    rewind(file);
    reserve((size_t)length);
    if (length > 0 && fread(content, 1, (size_t)length, file) != (size_t)length) die("read source");
    content_len = (size_t)length;
    fclose(file);
}

static void persist(void) {
    if (read_only || !capture_path) return;
    char temporary[4096];
    if (snprintf(temporary, sizeof temporary, "%s.tmp.%ld", capture_path, (long)getpid()) >= (int)sizeof temporary) {
        errno = ENAMETOOLONG;
        die("capture path");
    }
    int fd = open(temporary, O_WRONLY | O_CREAT | O_TRUNC, 0600);
    if (fd < 0) die("open capture");
    size_t written = 0;
    while (written < content_len) {
        ssize_t amount = write(fd, content + written, content_len - written);
        if (amount < 0) die("write capture");
        if (amount == 0) {
            errno = EIO;
            die("write capture made no progress");
        }
        written += (size_t)amount;
    }
    if (fsync(fd) != 0) die("sync capture");
    if (close(fd) != 0) die("close capture");
    if (rename(temporary, capture_path) != 0) die("publish capture");
}

static struct fuse_attr attr_for(uint64_t nodeid) {
    struct fuse_attr attr;
    memset(&attr, 0, sizeof attr);
    attr.ino = nodeid;
    attr.mode = nodeid == ROOT_INO ? (S_IFDIR | 0555) : (S_IFREG | (read_only ? 0444 : 0644));
    attr.nlink = nodeid == ROOT_INO ? 2 : 1;
    attr.uid = getuid();
    attr.gid = getgid();
    attr.size = nodeid == FILE_INO ? content_len : 0;
    attr.blocks = (attr.size + 511) / 512;
    attr.blksize = 4096;
    return attr;
}

static void reply_raw(int fuse, uint64_t unique, int error, const void *body, size_t body_len) {
    unsigned char out[BUFFER_BYTES];
    if (sizeof(struct fuse_out_header) + body_len > sizeof out) {
        errno = EOVERFLOW;
        die("reply too large");
    }
    struct fuse_out_header header = {
        .len = (uint32_t)(sizeof header + body_len),
        .error = error ? -error : 0,
        .unique = unique,
    };
    memcpy(out, &header, sizeof header);
    if (body_len) memcpy(out + sizeof header, body, body_len);
    if (write(fuse, out, header.len) != (ssize_t)header.len) die("reply");
}

static void reply_empty(int fuse, uint64_t unique) {
    reply_raw(fuse, unique, 0, NULL, 0);
}

static void reply_error(int fuse, uint64_t unique, int error) {
    reply_raw(fuse, unique, error, NULL, 0);
}

static bool add_dirent(unsigned char *out, size_t capacity, size_t *at, uint64_t ino, uint64_t off, uint32_t type, const char *name) {
    size_t name_len = strlen(name);
    size_t record_len = FUSE_DIRENT_ALIGN(FUSE_NAME_OFFSET + name_len);
    if (*at > capacity || record_len > capacity - *at) return false;
    struct fuse_dirent entry = {.ino = ino, .off = off, .namelen = (uint32_t)name_len, .type = type};
    memcpy(out + *at, &entry, FUSE_NAME_OFFSET);
    memcpy(out + *at + FUSE_NAME_OFFSET, name, name_len);
    memset(out + *at + FUSE_NAME_OFFSET + name_len, 0, record_len - FUSE_NAME_OFFSET - name_len);
    *at += record_len;
    return true;
}

static bool checked_content_end(uint64_t offset, uint32_t length, size_t *end) {
    if (offset > MAX_CONTENT_BYTES || length > MAX_CONTENT_BYTES - offset) return false;
    *end = (size_t)offset + length;
    return true;
}

static void dispatch(int fuse, const struct fuse_in_header *header, const unsigned char *body, size_t body_len, bool *done) {
    switch (header->opcode) {
    case FUSE_INIT: {
        if (body_len < sizeof(struct fuse_init_in)) {
            reply_error(fuse, header->unique, EPROTO);
            return;
        }
        struct fuse_init_in request;
        memcpy(&request, body, sizeof request);
        struct fuse_init_out result;
        memset(&result, 0, sizeof result);
        result.major = FUSE_KERNEL_VERSION;
        result.minor = request.minor < FUSE_KERNEL_MINOR_VERSION ? request.minor : FUSE_KERNEL_MINOR_VERSION;
        result.max_readahead = request.max_readahead;
        result.flags = request.flags & (FUSE_ASYNC_READ | FUSE_BIG_WRITES | FUSE_ATOMIC_O_TRUNC);
        result.max_background = 16;
        result.congestion_threshold = 12;
        result.max_write = 128 * 1024;
        result.time_gran = 1;
        reply_raw(fuse, header->unique, 0, &result, sizeof result);
        break;
    }
    case FUSE_LOOKUP: {
        size_t lookup_len = body_len ? strnlen((const char *)body, body_len) : 0;
        if (header->nodeid != ROOT_INO || lookup_len == 0 || lookup_len == body_len ||
            lookup_len != strlen(entry_name) || memcmp(body, entry_name, lookup_len) != 0) {
            reply_error(fuse, header->unique, ENOENT);
            return;
        }
        struct fuse_entry_out result;
        memset(&result, 0, sizeof result);
        result.nodeid = FILE_INO;
        result.generation = 1;
        result.entry_valid = 1;
        result.attr_valid = 1;
        result.attr = attr_for(FILE_INO);
        reply_raw(fuse, header->unique, 0, &result, sizeof result);
        break;
    }
    case FUSE_GETATTR: {
        if (header->nodeid != ROOT_INO && header->nodeid != FILE_INO) {
            reply_error(fuse, header->unique, ENOENT);
            return;
        }
        struct fuse_attr_out result;
        memset(&result, 0, sizeof result);
        result.attr_valid = 1;
        result.attr = attr_for(header->nodeid);
        reply_raw(fuse, header->unique, 0, &result, sizeof result);
        break;
    }
    case FUSE_SETATTR: {
        if (header->nodeid != FILE_INO) {
            reply_error(fuse, header->unique, ENOENT);
            return;
        }
        if (read_only) {
            reply_error(fuse, header->unique, EROFS);
            return;
        }
        if (body_len < sizeof(struct fuse_setattr_in)) {
            reply_error(fuse, header->unique, EPROTO);
            return;
        }
        struct fuse_setattr_in request;
        memcpy(&request, body, sizeof request);
        if (request.valid & FATTR_SIZE) {
            if (request.size > MAX_CONTENT_BYTES) {
                reply_error(fuse, header->unique, EFBIG);
                return;
            }
            reserve((size_t)request.size);
            if ((size_t)request.size > content_len) memset(content + content_len, 0, (size_t)request.size - content_len);
            content_len = (size_t)request.size;
        }
        struct fuse_attr_out result;
        memset(&result, 0, sizeof result);
        result.attr_valid = 1;
        result.attr = attr_for(FILE_INO);
        reply_raw(fuse, header->unique, 0, &result, sizeof result);
        break;
    }
    case FUSE_OPEN: {
        if (header->nodeid != FILE_INO || body_len < sizeof(struct fuse_open_in)) {
            reply_error(fuse, header->unique, ENOENT);
            return;
        }
        struct fuse_open_in request;
        memcpy(&request, body, sizeof request);
        if (read_only && ((request.flags & O_ACCMODE) != O_RDONLY)) {
            reply_error(fuse, header->unique, EROFS);
            return;
        }
        if (!read_only && (request.flags & O_TRUNC)) content_len = 0;
        struct fuse_open_out result = {.fh = 1, .open_flags = FOPEN_DIRECT_IO, .padding = 0};
        reply_raw(fuse, header->unique, 0, &result, sizeof result);
        break;
    }
    case FUSE_READ: {
        if (header->nodeid != FILE_INO || body_len < sizeof(struct fuse_read_in)) {
            reply_error(fuse, header->unique, ENOENT);
            return;
        }
        struct fuse_read_in request;
        memcpy(&request, body, sizeof request);
        size_t offset = request.offset < content_len ? (size_t)request.offset : content_len;
        size_t available = request.offset < content_len ? content_len - offset : 0;
        size_t amount = request.size < available ? request.size : available;
        reply_raw(fuse, header->unique, 0, amount ? content + offset : NULL, amount);
        break;
    }
    case FUSE_WRITE: {
        if (header->nodeid != FILE_INO || body_len < sizeof(struct fuse_write_in)) {
            reply_error(fuse, header->unique, EPROTO);
            return;
        }
        if (read_only) {
            reply_error(fuse, header->unique, EROFS);
            return;
        }
        struct fuse_write_in request;
        memcpy(&request, body, sizeof request);
        if (request.size > body_len - sizeof request) {
            reply_error(fuse, header->unique, EPROTO);
            return;
        }
        if (request.size == 0) {
            struct fuse_write_out result = {.size = 0, .padding = 0};
            reply_raw(fuse, header->unique, 0, &result, sizeof result);
            return;
        }
        size_t end;
        if (!checked_content_end(request.offset, request.size, &end)) {
            reply_error(fuse, header->unique, EFBIG);
            return;
        }
        size_t offset = (size_t)request.offset;
        reserve(end);
        if (offset > content_len) memset(content + content_len, 0, offset - content_len);
        memcpy(content + offset, body + sizeof request, request.size);
        if (end > content_len) content_len = end;
        struct fuse_write_out result = {.size = request.size, .padding = 0};
        reply_raw(fuse, header->unique, 0, &result, sizeof result);
        break;
    }
    case FUSE_FLUSH:
    case FUSE_FSYNC:
        persist();
        reply_empty(fuse, header->unique);
        break;
    case FUSE_RELEASE:
        persist();
        reply_empty(fuse, header->unique);
        break;
    case FUSE_OPENDIR: {
        if (header->nodeid != ROOT_INO) {
            reply_error(fuse, header->unique, ENOTDIR);
            return;
        }
        struct fuse_open_out result = {.fh = 2, .open_flags = 0, .padding = 0};
        reply_raw(fuse, header->unique, 0, &result, sizeof result);
        break;
    }
    case FUSE_READDIR: {
        if (header->nodeid != ROOT_INO || body_len < sizeof(struct fuse_read_in)) {
            reply_error(fuse, header->unique, ENOTDIR);
            return;
        }
        struct fuse_read_in request;
        memcpy(&request, body, sizeof request);
        unsigned char entries[1024];
        size_t capacity = request.size < sizeof entries ? request.size : sizeof entries;
        size_t used = 0;
        if (request.offset < 1 && !add_dirent(entries, capacity, &used, ROOT_INO, 1, DT_DIR, ".")) goto readdir_reply;
        if (request.offset < 2 && !add_dirent(entries, capacity, &used, ROOT_INO, 2, DT_DIR, "..")) goto readdir_reply;
        if (request.offset < 3) (void)add_dirent(entries, capacity, &used, FILE_INO, 3, DT_REG, entry_name);
readdir_reply:
        reply_raw(fuse, header->unique, 0, entries, used);
        break;
    }
    case FUSE_RELEASEDIR:
    case FUSE_STATFS: {
        if (header->opcode == FUSE_STATFS) {
            struct fuse_statfs_out result;
            memset(&result, 0, sizeof result);
            result.st.blocks = 1024 * 1024;
            result.st.bfree = result.st.bavail = result.st.blocks;
            result.st.files = 2;
            result.st.ffree = 1024;
            result.st.bsize = 4096;
            result.st.namelen = 255;
            result.st.frsize = 4096;
            reply_raw(fuse, header->unique, 0, &result, sizeof result);
        } else {
            reply_empty(fuse, header->unique);
        }
        break;
    }
    case FUSE_ACCESS: {
        struct fuse_access_in request;
        if (body_len >= sizeof request) memcpy(&request, body, sizeof request);
        if (read_only && body_len >= sizeof request && (request.mask & W_OK)) {
            reply_error(fuse, header->unique, EROFS);
        } else {
            reply_empty(fuse, header->unique);
        }
        break;
    }
    case FUSE_DESTROY:
        reply_empty(fuse, header->unique);
        *done = true;
        break;
    case FUSE_FORGET:
    case FUSE_BATCH_FORGET:
        break;
    default:
        reply_error(fuse, header->unique, ENOSYS);
        break;
    }
}

int main(int argc, char **argv) {
    if (argc != 6 || (strcmp(argv[5], "rw") != 0 && strcmp(argv[5], "ro") != 0)) {
        fprintf(stderr, "usage: %s <mountpoint> <entry-name> <source-file> <capture-file-or-dash> <rw|ro>\n", argv[0]);
        return 2;
    }
    const char *mountpoint = argv[1];
    entry_name = argv[2];
    size_t entry_name_len = strlen(entry_name);
    if (entry_name_len == 0 || entry_name_len > MAX_ENTRY_NAME_BYTES || strchr(entry_name, '/') ||
        strcmp(entry_name, ".") == 0 || strcmp(entry_name, "..") == 0) {
        fprintf(stderr, "entry name must be one path component of 1..%d bytes\n", MAX_ENTRY_NAME_BYTES);
        return 2;
    }
    load_file(argv[3]);
    capture_path = strcmp(argv[4], "-") == 0 ? NULL : argv[4];
    read_only = strcmp(argv[5], "ro") == 0;

    int fuse = open("/dev/fuse", O_RDWR);
    if (fuse < 0) die("open /dev/fuse");
    char options[256];
    snprintf(options, sizeof options, "fd=%d,rootmode=40000,user_id=%u,group_id=%u,max_read=131072", fuse, getuid(), getgid());
    if (mount("mesh-demo", mountpoint, "fuse", MS_NOSUID | MS_NODEV, options) != 0) die("mount fuse");
    printf("{\"ready\":true,\"mountpoint\":\"%s\",\"read_only\":%s}\n", mountpoint, read_only ? "true" : "false");
    fflush(stdout);

    unsigned char request[BUFFER_BYTES];
    bool done = false;
    while (!done) {
        ssize_t length = read(fuse, request, sizeof request);
        if (length < 0 && (errno == EINTR || errno == EAGAIN)) continue;
        if (length < 0 && errno == ENODEV) break;
        if (length < (ssize_t)sizeof(struct fuse_in_header)) die("read fuse request");
        size_t at = 0;
        while (at + sizeof(struct fuse_in_header) <= (size_t)length) {
            struct fuse_in_header header;
            memcpy(&header, request + at, sizeof header);
            if (header.len < sizeof header || at + header.len > (size_t)length) die("malformed fuse request");
            dispatch(fuse, &header, request + at + sizeof header, header.len - sizeof header, &done);
            at += header.len;
        }
        if (at != (size_t)length) die("trailing fuse request bytes");
    }
    persist();
    close(fuse);
    free(content);
    return 0;
}
