# Uncomment the following line to use OpenMP 2.0 features
#OMPFLAG = -DOMPVER2
# Uncomment the following line to use OpenMP 3.0 features
OMPFLAG = -DDISABLE_BARRIER_TEST -DOMPVER2 -DOMPVER3

CC = /usr/local/bin/gcc-4.6 
CFLAGS = -fopenmp -O1 -lm
LDFLAGS = -fopenmp -O1 -lm
CPP = /usr/bin/cpp
LIBS = 