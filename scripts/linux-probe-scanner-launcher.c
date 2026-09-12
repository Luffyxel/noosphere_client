#include <errno.h>
#include <limits.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

int main(int argc, char **argv) {
  char executable[PATH_MAX];
  ssize_t length = readlink("/proc/self/exe", executable, sizeof(executable) - 1);
  if (length <= 0 || length >= (ssize_t)sizeof(executable)) {
    perror("readlink");
    return 127;
  }
  executable[length] = '\0';
  char *separator = strrchr(executable, '/');
  if (separator == NULL) {
    return 127;
  }
  *separator = '\0';

  char loader[PATH_MAX];
  char library_path[PATH_MAX];
  char scanner[PATH_MAX];
  if (snprintf(loader, sizeof(loader), "%s/../lib/ld-linux-x86-64.so.2", executable) >=
          (int)sizeof(loader) ||
      snprintf(library_path, sizeof(library_path), "%s/../lib", executable) >=
          (int)sizeof(library_path) ||
      snprintf(scanner, sizeof(scanner), "%s/gst-plugin-scanner.real", executable) >=
          (int)sizeof(scanner)) {
    return 127;
  }

  char **arguments = calloc((size_t)argc + 4, sizeof(char *));
  if (arguments == NULL) {
    return 127;
  }
  arguments[0] = loader;
  arguments[1] = "--library-path";
  arguments[2] = library_path;
  arguments[3] = scanner;
  for (int index = 1; index < argc; ++index) {
    arguments[index + 3] = argv[index];
  }
  arguments[argc + 3] = NULL;
  execv(loader, arguments);
  fprintf(stderr, "Noosphere plugin scanner: %s\n", strerror(errno));
  free(arguments);
  return 127;
}
