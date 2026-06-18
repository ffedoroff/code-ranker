// Main.cs — entrypoint.
using System;
using Sample.Mathx;

namespace Sample {
	class Program {
		// Main sums squares of even positive numbers via a lambda.
		static int Main() {
			Func<int, bool> contributes = i => i % 2 == 0 && i > 0;
			int total = 0;
			for (int i = 1; i <= 10; i++) {
				if (contributes(i)) {
					total += Squarer.SquareClamped(i);
				}
			}
			Console.WriteLine(total);
			return total;
		}
	}
}
