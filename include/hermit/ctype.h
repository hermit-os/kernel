/****************************************************************************************
 *
 * Author: Stefan Lankes
 *         Chair for Operating Systems, RWTH Aachen University
 * Date:   24/03/2011
 *
 ****************************************************************************************
 * 
 * Written by the Chair for Operating Systems, RWTH Aachen University
 * 
 * NO Copyright (C) 2010, Stefan Lankes,
 * consider these trivial functions to be public domain.
 * 
 * These functions are distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 */ 

/** 
 * @author Stefan Lankes
 * @file include/hermit/ctype.h
 * @brief Functions related to alphanumerical character values
 *
 * This file contains functions helping to determine 
 * the type of alphanumerical character values.
 */

#ifndef __CTYPE_H_
#define __CYTPE_H_

/** Returns true if the value of 'c' is an ASCII-charater */
static inline int isascii(int c) 
{
	return (((unsigned char)(c))<=0x7f);
}

/** Applies an and-operation to 
 * push the value of 'c' into the ASCII-range */
static inline int toascii(int c)
{
	return (((unsigned char)(c))&0x7f);
}

/** Returns true if the value of 'c' is the 
 * space character or a control character */
static inline int isspace(int c)
{
	if (!isascii(c))
		return 0;

	if (' ' == (unsigned char) c)
		return 1;
	if ('\n' == (unsigned char) c)
		return 1;
	if ('\r' == (unsigned char) c)
		return 1;
	if ('\t' == (unsigned char) c)
		return 1;
	if ('\v' == (unsigned char) c)
		return 1;
	if ('\f' == (unsigned char) c)
		return 1;

	return 0;
}

/** Returns true if the value of 'c' is a number */
static inline int isdigit(int c)
{
	if (!isascii(c))
		return 0;

	if (((unsigned char) c >= '0') && ((unsigned char) c <= '9'))
		return 1;

	return 0;
}

/** Returns true if the value of 'c' is a lower case letter */
static inline int islower(int c)
{
	if (!isascii(c))
		return 0;

	if (((unsigned char) c >= 'a') && ((unsigned char) c <= 'z'))
		return 1;

	return 0;
}

/** Returns true if the value of 'c' is an upper case letter */
static inline int isupper(int c)
{
	if (!isascii(c))
		return 0;

	if (((unsigned char) c >= 'A') && ((unsigned char) c <= 'Z'))
		return 1;

	return 0;
}

/** Returns true if the value of 'c' is an alphabetic character */
static inline int isalpha(int c)
{
	if (isupper(c) || islower(c))
		return 1;

	return 0;
}

/** Makes the input character lower case.\n Will do nothing if it 
 * was something different than an upper case letter before. */
static inline unsigned char tolower(unsigned char c)
{
	if (isupper(c))
		c -= 'A'-'a';
	return c;
}

/** Makes the input character upper case.\n Will do nothing if it 
 * was something different than a lower case letter before. */
static inline unsigned char toupper(unsigned char c)
{
	if (islower(c))
		c -= 'a'-'A';
	return c;
}

#endif
