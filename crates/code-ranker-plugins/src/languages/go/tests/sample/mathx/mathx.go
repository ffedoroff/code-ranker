// Package mathx provides small math helpers for the sample.
package mathx

import "example.com/sample/util"

// Square returns n*n, clamped to be at least zero via util.Max.
func Square(n int) int {
	v := n * n
	return util.Max(v, 0)
}
