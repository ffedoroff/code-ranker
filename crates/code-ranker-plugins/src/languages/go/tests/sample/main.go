// Package main is the sample entrypoint for the code-ranker Go fixture.
package main

import (
	"fmt"

	"example.com/sample/mathx"
)

// main sums the squares of the even numbers in 1..10 and prints the total.
func main() {
	// A closure that doubles its argument.
	double := func(x int) int { return x * 2 }

	total := 0
	for i := 1; i <= 10; i++ {
		// Only even, positive values contribute.
		if i%2 == 0 && i > 0 {
			total += mathx.Square(i)
		}
	}
	fmt.Println(double(total))
}
