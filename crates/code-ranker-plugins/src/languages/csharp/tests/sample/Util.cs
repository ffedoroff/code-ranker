// Util.cs — tiny helpers.
namespace Sample.Util {
	public static class Maths {
		// Max returns the larger of a and b.
		public static int Max(int a, int b) {
			if (a > b) {
				return a;
			}
			return b;
		}
	}
}
