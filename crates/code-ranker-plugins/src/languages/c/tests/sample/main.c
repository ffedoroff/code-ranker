#include <stdio.h>
#include "mathx.h"

// main sums the squares of even positive numbers in 1..10.
int main(void) {
	int total = 0;
	for (int i = 1; i <= 10; i++) {
		// only even, positive values contribute
		if (i % 2 == 0 && i > 0) {
			total += square_clamped(i);
		}
	}
	printf("%d\n", total);
	return 0;
}
