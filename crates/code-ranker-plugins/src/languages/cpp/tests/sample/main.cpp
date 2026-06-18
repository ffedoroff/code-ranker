#include <vector>
#include "mathx.hpp"

// main sums squares of even positive numbers in 1..10 via a lambda.
int main() {
	auto contributes = [](int i) { return i % 2 == 0 && i > 0; };
	int total = 0;
	for (int i = 1; i <= 10; i++) {
		if (contributes(i)) {
			total += square_clamped(i);
		}
	}
	return total;
}
