#ifndef GPufBridge_h
#define GPufBridge_h

#if !defined(__ANDROID__)
typedef signed char jbyte;
typedef unsigned short jchar;
typedef short jshort;
typedef int jint;
typedef long long jlong;
typedef float jfloat;
typedef double jdouble;
typedef unsigned char jboolean;

typedef void *JNIEnv;
typedef void *JClass;
typedef void *JObject;
typedef void *jobject;
typedef void *jstring;
typedef void *JString;
#endif

#include "gpuf_c.h"

#endif /* GPufBridge_h */
