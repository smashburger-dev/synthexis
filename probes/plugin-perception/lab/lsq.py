"""Least squares without numpy (normal equations, small ridge)."""


def solve(A, b):
    n = len(A)
    M = [row[:] + [b[i]] for i, row in enumerate(A)]
    for c in range(n):
        p = max(range(c, n), key=lambda r: abs(M[r][c]))
        M[c], M[p] = M[p], M[c]
        if abs(M[c][c]) < 1e-12:
            continue
        for r in range(n):
            if r != c:
                f = M[r][c] / M[c][c]
                M[r] = [x - f * y for x, y in zip(M[r], M[c])]
    return [M[i][n] / M[i][i] if abs(M[i][i]) > 1e-12 else 0.0 for i in range(n)]


def fit(X, Y, ridge=1e-6):
    k = len(X[0]) + 1
    A = [[0.0] * k for _ in range(k)]
    b = [0.0] * k
    for x, y in zip(X, Y):
        z = [1.0] + list(x)
        for i in range(k):
            b[i] += z[i] * y
            for j in range(k):
                A[i][j] += z[i] * z[j]
    for i in range(1, k):
        A[i][i] += ridge * len(X)
    return solve(A, b)


def predict(w, x):
    return w[0] + sum(a * b for a, b in zip(w[1:], x))
