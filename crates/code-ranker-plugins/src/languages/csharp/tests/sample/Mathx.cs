// Mathx.cs — depends on Sample.Util.
using Sample.Util;

namespace Sample.Mathx {
	public static class Squarer {
		// SquareClamped returns max(n*n, 0).
		public static int SquareClamped(int n) {
			int v = n * n;
			return Maths.Max(v, 0);
		}
	}
}
