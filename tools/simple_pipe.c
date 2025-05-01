#include <stdio.h>
#include <unistd.h>
#include <linux/input.h>
#include <errno.h>
#include <string.h> // For strerror

int main() {
    struct input_event ev;
    ssize_t bytes_read;
    ssize_t bytes_written;
    const size_t event_size = sizeof(struct input_event);

    while (1) {
        // Read one input_event from stdin (fd 0)
        bytes_read = read(STDIN_FILENO, &ev, event_size);

        if (bytes_read == 0) {
            // End of file
            break;
        } else if (bytes_read < 0) {
            // Error during read
            if (errno == EINTR) {
                continue; // Interrupted by signal, try again
            }
            perror("Error reading from stdin");
            return 1;
        } else if ((size_t)bytes_read < event_size) {
            // Partial read (should not happen with pipes, but check anyway)
            fprintf(stderr, "Error: Partial read from stdin (%zd bytes)\n", bytes_read);
            return 1;
        }

        // Log the received event to stderr
        fprintf(stderr, "Read event: time=%ld.%06ld, type=%d, code=%d, value=%d\n",
                (long)ev.time.tv_sec, (long)ev.time.tv_usec,
                ev.type, ev.code, ev.value);

        // Write the exact same event to stdout (fd 1)
        bytes_written = write(STDOUT_FILENO, &ev, event_size);

        if (bytes_written < 0) {
            // Error during write
            if (errno == EPIPE) {
                // Broken pipe (downstream closed), exit gracefully
                fprintf(stderr, "Simple_pipe: Output pipe broken, exiting.\n");
                break;
            } else if (errno == EINTR) {
                 // Interrupted by signal, attempt to rewrite the *same* event
                 // Need a loop here to handle repeated EINTR on write
                 ssize_t total_written = 0;
                 while (total_written < (ssize_t)event_size) {
                     bytes_written = write(STDOUT_FILENO, ((char*)&ev) + total_written, event_size - total_written);
                     if (bytes_written < 0) {
                         if (errno == EINTR) {
                             continue; // Try write again
                         } else if (errno == EPIPE) {
                             fprintf(stderr, "Simple_pipe: Output pipe broken during retry, exiting.\n");
                             goto end_loop; // Use goto to break out of nested loops cleanly
                         } else {
                             perror("Error writing to stdout during retry");
                             return 1;
                         }
                     }
                     total_written += bytes_written;
                 }
                 continue; // Continue outer loop after successful retry
            } else {
                perror("Error writing to stdout");
                return 1;
            }
        } else if ((size_t)bytes_written < event_size) {
            // Partial write (should not happen with pipes, but check anyway)
            fprintf(stderr, "Error: Partial write to stdout (%zd bytes)\n", bytes_written);
            // Consider retrying partial writes if necessary, but for pipes it usually indicates an error.
            return 1;
        }
    }

end_loop: // Label for breaking out cleanly on EPIPE during write retry
    return 0; // Success
}
