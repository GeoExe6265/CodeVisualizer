def bubble_sort(values):
    n = len(values)
    for i in range(n - 1):
        swapped = False
        for j in range(n - 1 - i):
            if values[j] > values[j + 1]:
                values[j], values[j + 1] = values[j + 1], values[j]
                swapped = True
        if not swapped:
            break
    return values


def fib(n):
    if n <= 1:
        return n
    return fib(n - 1) + fib(n - 2)


numbers = [5, 2, 9, 1, 7, 3]
print("До сортировки:", numbers)
sorted_numbers = bubble_sort(numbers)
print("После сортировки:", sorted_numbers)
print("fib(5) =", fib(5))
