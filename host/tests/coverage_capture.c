/* T497: callback contracts and bounded parser fuzzing using fake EVDI only.
 * Included by the existing helper harness; no libevdi or host device access. */
static void t497_dispatch(struct evdi_event_context *context) {
    assert(context->user_data == &g_capture);
    if (t497_event_count == 0) {
        struct evdi_mode mode = {8, 8, 60, 32, 0x34325258};
        context->mode_changed_handler(mode, context->user_data);
    }
    context->dpms_handler(t497_event_count % 2, context->user_data);
    context->crtc_state_handler(t497_event_count % 2, context->user_data);
    context->cursor_set_handler((struct evdi_cursor_set){0}, context->user_data);
    context->cursor_move_handler((struct evdi_cursor_move){0}, context->user_data);
    mock_monotonic_ms += 1000;
    context->update_ready_handler(0, context->user_data);
    if (++t497_event_count == 6) g_running = 0;
}

static void t497_free_capture(void) {
    free(g_capture.framebuffer);
    g_capture.framebuffer = NULL;
    frame_exchange_free(&g_frames);
    conv_pool_destroy(&g_conversion);
    pthread_cond_destroy(&g_frames.ready);
}

static void test_t497_callbacks(void) {
    frame_exchange_init(&g_frames);
    t497_capture_events = 1;
    mock_monotonic_ms = 0;
    g_running = 1;
    int channel[2];
    assert(pipe(channel) == 0 && write(channel[1], "x", 1) == 1);
    struct evdi_device_context handle = {.fd = channel[0]};
    g_capture.handle = &handle;
    t497_immediate = 1;
    t497_rectangles = 1;
    g_frames.latency_count = 3;
    g_frames.latency[0] = 3000; g_frames.latency[1] = 1000; g_frames.latency[2] = 2000;
    assert(capture_run(&g_capture, &handle) == 0);
    assert(g_capture.have_mode && g_frames.latest_valid && g_capture.grab_count > 0);
    assert(g_frames.latency_count == 0 && t497_event_count == 6);
    t497_rectangles = 128; /* Report overflow without writing past the driver buffer. */
    grab_now(&g_capture);
    assert(g_frames.latest_valid);
    g_capture.update_pending = 0;
    t497_immediate = 0;
    request_capture_if_due(&g_capture, &handle, 10000, 16);
    assert(g_capture.update_pending);
    long long fallback = 0;
    recover_capture_if_stalled(&g_capture, 11000, &fallback);
    assert(!g_capture.update_pending && fallback == 11000);
    writer_state_t state = {.last_write_ms = 6000};
    g_frames.writer_busy = 1;
    assert(!writer_frame_due(&g_writer, &state, 0));
    assert(!g_frames.writer_busy);
    mock_monotonic_ms += IDLE_KEEPALIVE_MS;
    assert(writer_frame_due(&g_writer, &state, 0));
    close(channel[0]); close(channel[1]);
    t497_free_capture();
}

static void test_t497_main(void) {
    char root[] = "/tmp/uscreen-t497-main-XXXXXX", edid[4096];
    assert(mkdtemp(root));
    snprintf(mock_card_root, sizeof(mock_card_root), "%s", root);
    make_test_card(root, 17);
    snprintf(edid, sizeof(edid), "%s/edid", root);
    FILE *file = fopen(edid, "wb"); assert(file);
    unsigned char block[128] = {0};
    assert(fwrite(block, 1, sizeof(block), file) == sizeof(block));
    assert(fclose(file) == 0);
    t497_capture_events = 1; t497_immediate = 0; t497_rectangles = 0;
    mock_monotonic_ms = 0;
    g_running = 1;
    int found = -1;
    evdi_handle available = wait_for_available_device(root, 1, &found);
    assert(available != EVDI_INVALID_HANDLE && found == 17);
    evdi_close(available);
    long size = 0;
    allocation_countdown = 1;
    assert(read_edid_file(edid, &size) == NULL && size == 0);
    allocation_countdown = 0;
    char *args[] = {"helper", "--edid", edid, "--conversion-threads", "128"};
    assert(evdi_helper_main(5, args) == 0);
    assert(mock_disconnect_calls == 1 && t497_event_count == 6);
    assert(g_conversion_threads == 128);
    unlink(edid);
    char path[4096];
    snprintf(path, sizeof(path), "%s/card17", root); unlink(path);
    snprintf(path, sizeof(path), "%s/evdi.17/drm/card17", root); rmdir(path);
    snprintf(path, sizeof(path), "%s/evdi.17/drm", root); rmdir(path);
    snprintf(path, sizeof(path), "%s/evdi.17", root); rmdir(path);
    add_result = 1;
    assert(acquire_capture_device_in(root, &found) == EVDI_INVALID_HANDLE);
    assert(mock_add_calls == 1);
    g_running = 1;
    assert(wait_for_available_device(root, 1, &found) == EVDI_INVALID_HANDLE);
    rmdir(root);
}

static void t497_mutate_capacities(void) {
    unsigned seed = 497;
    for (int i = 0; i < 2048; i++) {
        char text[65];
        for (unsigned j = 0; j < sizeof(text) - 1; j++) {
            seed = seed * 1664525u + 1013904223u;
            text[j] = (char)(33 + seed % 94);
        }
        text[64] = 0;
        int value = conversion_capacity(text);
        assert(value >= 0 && value <= MAX_CONV_THREADS);
    }
}

static void t497_writer_startup(void) {
    frame_exchange_init(&g_frames);
    g_running = 0; /* Real thread exits immediately; never opens the sentinel path. */
    g_conversion_threads = 2;
    pthread_t writer = 0;
    t497_fail_writer = 1;
    assert(!start_capture_writer("/unopened-t497-fifo", &writer));
    assert(!writer && g_fifo.fd == -1);
    conv_pool_destroy(&g_conversion);
    t497_fail_writer = 0;
    assert(start_capture_writer("/unopened-t497-fifo", &writer));
    assert(pthread_join(writer, NULL) == 0 && g_fifo.fd == -1);
    conv_pool_destroy(&g_conversion);
    pthread_cond_destroy(&g_frames.ready);
}

static void t497_pacing_and_poll_errors(void) {
    struct timespec deadline = {.tv_sec = 5, .tv_nsec = 999999999};
    add_period(&deadline, 1000000002L);
    assert(deadline.tv_sec == 7 && deadline.tv_nsec == 1);
    struct pollfd fd = {.fd = -1};
    struct evdi_event_context context = {0};
    t497_poll_error = EINTR;
    assert(poll_capture_events(EVDI_INVALID_HANDLE, &context, &fd, 0) == 0);
    t497_poll_error = EIO;
    assert(poll_capture_events(EVDI_INVALID_HANDLE, &context, &fd, 0) == -1);
    t497_poll_error = 0;
}

static void test_t497_bounds(void) {
    g_running = 1;
    g_fifo.fd = -1;
    retire_partial_fifo(&g_fifo);
    assert(!g_running && !g_fifo.retired && g_fifo.fd == -1);
    /* Numeric capacities never wrap into an accepted worker count. */
    const char *invalid[] = {"", "-1", "129", "1junk", "128\n", "9999999999999999999999999999999999"};
    for (unsigned i = 0; i < sizeof(invalid) / sizeof(invalid[0]); i++)
        assert(conversion_capacity(invalid[i]) == 0);
    for (int n = -1024; n < 1024; n++) {
        char text[32]; snprintf(text, sizeof(text), "%d", n);
        int expected = n >= 0 && n <= MAX_CONV_THREADS ? n : 0;
        assert(conversion_capacity(text) == expected);
    }
    t497_mutate_capacities();
    t497_writer_startup();
    t497_pacing_and_poll_errors();
}
