static void t492_invalid_control(int fifo) {
    char text[160];
    struct stat info; assert(fstat(fifo, &info) == 0);
    const char *invalid[] = {"", "-1 1 1 1200 500\n", "1", "1 1 1 1200 500X",
        "184467440737095516160000 1 1 1200 500\n", "1 1 1 1200 4294967796\n"};
    for (size_t i = 0; i < sizeof(invalid)/sizeof(invalid[0]); i++) {
        strcpy(text, invalid[i]);
        assert(idle_control_value(text, strlen(text), fifo, 1000) == 200);
    }
    for (int delta = -2; delta <= 2002; delta++) {
        int size = snprintf(text, sizeof(text), "%ju %ju 1 %d 500\n",
                            (uintmax_t)info.st_dev, (uintmax_t)info.st_ino, 1000 + delta);
        assert(idle_control_value(text, size, fifo, 1000) == ((delta > 0 && delta <= 2000) ? 500 : 200));
    }
    strcpy(text, "1 2 3 1200 500\n");
    assert(idle_control_value(text, strlen(text), fifo, 1000) == 200);
    assert(idle_control_value(text, strlen(text), -1, 1000) == 200);
    for (int size = -2; size <= 162; size++) {
        memset(text, '9', sizeof(text));
        assert(idle_control_value(text, size, fifo, 1000) == 200);
    }
}

static void t492_file_restrictions(const char *path) {
    char text[160];
    assert(read_idle_control(NULL, text, sizeof(text)) < 0);
    assert(read_idle_control("/nonexistent/t492", text, sizeof(text)) < 0);
    assert(chmod(path, 0666) == 0);
    assert(read_idle_control(path, text, sizeof(text)) < 0);
    assert(chmod(path, 0600) == 0);
    char link[256]; snprintf(link, sizeof(link), "%s-link", path);
    assert(symlink(path, link) == 0);
    assert(read_idle_control(link, text, sizeof(text)) < 0);
    unlink(link);
    assert(read_idle_control("/tmp", text, sizeof(text)) < 0);
}

static void t492_sparse_deadlines(writer_state_t *state, long long now) {
    mock_monotonic_ms = now + 499;
    state->last_write_ms = now;
    assert(!writer_frame_due(&g_writer, state, 0));
    mock_monotonic_ms++;
    assert(writer_frame_due(&g_writer, state, 0));
    mock_monotonic_ms++;
    assert(writer_frame_due(&g_writer, state, 1));
    mock_monotonic_ms = now + 1501;
    state->last_write_ms = now + 1200;
    assert(writer_frame_due(&g_writer, state, 0));
    mock_monotonic_ms = -1;
}

/* T492: a request is bounded to one FIFO inode and a short monotonic lease. */
static void test_t492_idle(void) {
    char path[] = "/tmp/uscreen-t492-idle-XXXXXX";
    int file = mkstemp(path); assert(file >= 0);
    int ends[2]; assert(pipe(ends) == 0);
    struct stat identity; assert(fstat(ends[1], &identity) == 0);
    char text[160];
    long long now = writer_now_ms();
    int size = snprintf(text, sizeof(text), "%ju %ju 1 %lld 500\n", (uintmax_t)identity.st_dev,
                        (uintmax_t)identity.st_ino, now + 1500);
    assert(write(file, text, size) == size); close(file);
    char *args[] = {"helper", "--idle-control-file", path};
    parse_helper_options(3, args);
    g_fifo.fd = ends[1];
    writer_state_t state = {.last_write_ms = now - 250};
    frame_exchange_init(&g_frames);
    assert(!writer_frame_due(&g_writer, &state, 0) && "T492: valid sparse lease still sends five idle frames/s");
    assert(writer_frame_due(&g_writer, &state, 1) && "T492: fresh damage must bypass idle deadline");
    t492_sparse_deadlines(&state, now);
    t492_invalid_control(ends[1]);
    t492_file_restrictions(path);
    assert(unlink(path) == 0);
    state.last_write_ms = writer_now_ms() - 250;
    assert(writer_frame_due(&g_writer, &state, 0));
    close(ends[0]); close(ends[1]); g_fifo.fd = -1; unlink(path);
}
