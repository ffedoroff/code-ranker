#include "mathx.hpp"

// square_clamped returns max(n*n, 0) using u_max.
int square_clamped(int n) {
	int v = n * n;
	return u_max(v, 0);
}
