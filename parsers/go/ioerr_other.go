//go:build !windows

package synx

import (
	"errors"
	"syscall"
)

// invalidFileName: the platform cannot hold this name at all (a NUL in it).
func invalidFileName(err error) bool {
	return errors.Is(err, syscall.EINVAL)
}
