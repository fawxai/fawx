package ai.citros.core

class BudgetExceededException(
    message: String,
    val code: BudgetErrorCode? = null
) : RuntimeException(message)
