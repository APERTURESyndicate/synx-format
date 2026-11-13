//go:build windows

package synx

import (
	"errors"
	"syscall"
)

// invalidFileName: the platform cannot hold this name at all —
// ERROR_INVALID_NAME, ERROR_FILENAME_EXCED_RANGE, or a NUL in it.
func invalidFileName(err error) bool {
	var errno syscall.Errno
	if !errors.As(err, &errno) {
		return false
	}
	return errno == 123 || errno == 206 || errno == syscall.EINVAL
}
