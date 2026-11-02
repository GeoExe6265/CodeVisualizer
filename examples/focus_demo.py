values = [10, 20, 30]
prices = {"apple": 3, "pear": 5}
index = 1
name = "pear"
current = values[index]
price = prices[name]
values[0], values[2] = values[2], values[0]
values.append(40)
print(current, price)
