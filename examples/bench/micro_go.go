package main

import (
	"fmt"
	"time"
)

func fib(n int) int {
	if n < 2 {
		return n
	}
	return fib(n-1) + fib(n-2)
}

func opaqueAdd(a, b int) int {
	t := time.Now().UnixNano()
	return a + b + int(t-t)
}

func benchFib() int {
	n := opaqueAdd(30, 0)
	iters := opaqueAdd(5, 0)
	acc := 0
	for i := 0; i < iters; i++ {
		acc += fib(n)
	}
	return acc
}

func benchSlice() int {
	n := opaqueAdd(100000, 0)
	a := make([]int, 0, n)
	for i := 0; i < n; i++ {
		a = append(a, i)
	}
	return len(a)
}

func benchMap() int {
	n := opaqueAdd(50000, 0)
	m := make(map[int]int, n)
	for i := 0; i < n; i++ {
		m[i] = i * 2
	}
	sum := 0
	for i := 0; i < n; i++ {
		sum += m[i]
	}
	return sum
}

func main() {
	_ = benchFib()
	_ = benchSlice()
	_ = benchMap()

	t0 := time.Now()
	f := benchFib()
	t1 := time.Now()
	s := benchSlice()
	t2 := time.Now()
	m := benchMap()
	t3 := time.Now()

	fmt.Println("lang")
	fmt.Println("go")
	fmt.Println("fib30x5")
	fmt.Println(f)
	fmt.Println(t1.Sub(t0).Nanoseconds())
	fmt.Println("slice100k")
	fmt.Println(s)
	fmt.Println(t2.Sub(t1).Nanoseconds())
	fmt.Println("map50k")
	fmt.Println(m)
	fmt.Println(t3.Sub(t2).Nanoseconds())
}
